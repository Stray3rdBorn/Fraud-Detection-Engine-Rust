// the shared state, the time limit helper and the clock of the server.
use crate::{AppState, error::ApiError, unix_now, validate, within};
// pieces for reading the path and the query, and for the JSON body and answer.
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
// the way request bodies are read.
use serde::Deserialize;
// shared pointer to the state.
use std::sync::Arc;
// the case types, the Postgres store, the key bundle and the storage contract.
use store::{CaseRow, CaseTarget, Keys, Postgres, Store};

// most cases returned by one list request.
const MAX_PAGE: i64 = 100;
// cases returned when no limit is given.
const DEFAULT_PAGE: i64 = 50;

// the two answers an analyst can give.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    // the transaction was fraud.
    Fraud,
    // the engine was wrong, the transaction was fine.
    FalseAlarm,
}

impl Verdict {
    // the word stored in the database.
    fn as_str(self) -> &'static str {
        // one word per verdict.
        match self {
            Verdict::Fraud => "fraud",
            Verdict::FalseAlarm => "false_alarm",
        }
    }
}

// the body of a verdict request, an unknown field is an error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictRequest {
    // the answer.
    verdict: Verdict,
    // an optional remark, kept with the verdict.
    note: Option<String>,
}

// the query of a list request.
#[derive(Deserialize)]
pub struct ListParams {
    // only cases after this case number are returned, empty means from the start.
    after: Option<i64>,
    // how many cases to return.
    limit: Option<i64>,
}

// the database, or an error when none is configured.
fn database<S>(app: &AppState<S>) -> Result<&Postgres, ApiError> {
    // the routes exist only with a database, this is a second line of defense.
    app.db.as_ref().ok_or(ApiError::Unavailable)
}

// lists open cases, oldest first.
pub async fn list<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the query parameters.
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<CaseRow>>, ApiError> {
    // the database.
    let db = database(&app)?;
    // a negative start counts as the start, the page size stays between 1 and the maximum.
    let after = params.after.unwrap_or(0).max(0);
    // the page size.
    let limit = params.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    // read within the time limit.
    let cases = within(app.settings.request_timeout, async {
        // a database error becomes the same answer as any other store failure.
        Ok::<_, ApiError>(db.open_cases(after, limit).await?)
    })
    .await?;
    // send them as JSON.
    Ok(Json(cases))
}

// changes the blocklist or the allowlist according to a verdict, all three records under their locks.
async fn apply_verdict<S: Store>(
    app: &AppState<S>,
    target: &CaseTarget,
    verdict: Verdict,
    now: i64,
) -> Result<(), ApiError> {
    // the same locks as a decision, so a verdict never mixes with a transaction on the same records.
    let _guards = app
        .locks
        .lock(&[&target.user_id, &target.device_id, &target.ip])
        .await;
    // the ids of the records.
    let keys = Keys {
        user_id: &target.user_id,
        device_id: &target.device_id,
        ip: &target.ip,
        event_id: &target.event_id,
    };
    // read the current records, the stored decision is not needed.
    let (mut state, mut entities, _) = app.store.load(&keys).await?;
    // the durations come from the settings.
    let config = app.engine.config();
    // the verdict decides what changes.
    match verdict {
        // fraud blocks the account, the device and the IP, and removes any allowlist entry.
        Verdict::Fraud => {
            // the end of the new block.
            let until = now.saturating_add(config.verdict_block_seconds);
            // an existing longer block is never shortened.
            state.blocked_until = state.blocked_until.max(Some(until));
            // the device.
            entities.device.blocked_until = entities.device.blocked_until.max(Some(until));
            // the IP.
            entities.ip.blocked_until = entities.ip.blocked_until.max(Some(until));
            // a confirmed fraud cannot stay cleared.
            state.allowed_until = None;
        }
        // a false alarm only lowers the score of the account for a while.
        Verdict::FalseAlarm => {
            // the end of the allowlist entry, the blocklist still wins over it.
            state.allowed_until = Some(now.saturating_add(config.verdict_allow_seconds));
        }
    }
    // save the records.
    app.store.update(&keys, state, entities).await?;
    // done.
    Ok(())
}

// does the whole verdict: find the case, change the records, then close the case.
async fn record_verdict<S: Store>(
    app: &AppState<S>,
    db: &Postgres,
    case_id: i64,
    request: &VerdictRequest,
) -> Result<(), ApiError> {
    // a missing or closed case ends here.
    let target = db.case_target(case_id).await?.ok_or(ApiError::NotFound)?;
    // the records change first, so a failure here leaves the case open and the verdict can be sent again.
    apply_verdict(app, &target, request.verdict, unix_now()).await?;
    // closing stores the verdict, false means another verdict closed the case in the meantime.
    if !db
        .close_case(case_id, request.verdict.as_str(), request.note.as_deref())
        .await?
    {
        // report it like a missing case.
        return Err(ApiError::NotFound);
    }
    // done.
    Ok(())
}

// handles one verdict: check the input, change the records, close the case.
pub async fn verdict<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the case number from the path.
    Path(case_id): Path<i64>,
    // the verdict, parsed from the JSON body.
    Json(request): Json<VerdictRequest>,
) -> Result<StatusCode, ApiError> {
    // the database.
    let db = database(&app)?;
    // refuse a bad note before any work is done.
    validate::note(request.note.as_deref())?;
    // do the work within the time limit.
    within(
        app.settings.request_timeout,
        record_verdict(&app, db, case_id, &request),
    )
    .await?;
    // nothing to return.
    Ok(StatusCode::NO_CONTENT)
}

// tests for the verdicts.
#[cfg(test)]
mod tests {
    // bring the functions and the shared test helpers into the tests.
    use super::*;
    use crate::{
        decide,
        testing::{app, event},
    };
    use engine::Outcome;

    // the time of the test transactions, also used as the time of the verdict.
    const NOW: i64 = 1_700_000_000;

    // the ids of the first test transaction, as the database would return them.
    fn target() -> CaseTarget {
        // same ids as the test event.
        CaseTarget {
            event_id: "e1".to_string(),
            user_id: "u1".to_string(),
            device_id: "d1".to_string(),
            ip: "1.2.3.4".to_string(),
        }
    }

    // a fraud verdict blocks the account and everyone who shares its device.
    #[tokio::test]
    async fn a_fraud_verdict_blocks_the_account_and_its_device() {
        // a fresh app and one transaction.
        let app = app();
        // the transaction the case is about.
        decide(&app, &event("e1")).await.unwrap();
        // the analyst says fraud.
        apply_verdict(&app, &target(), Verdict::Fraud, NOW).await.unwrap();
        // the same account is blocked now.
        let same = decide(&app, &event("e2")).await.unwrap();
        // the block outcome and the reason.
        assert!(matches!(same.outcome, Outcome::Block));
        // the blocklist reason is there.
        assert!(same.reasons.iter().any(|r| r.starts_with("blocklist")));
        // another account on the same device is blocked too.
        let mut other = event("e3");
        // a different user, the same device and IP.
        other.user_id = "u2".to_string();
        // decide.
        let shared = decide(&app, &other).await.unwrap();
        // blocked as well.
        assert!(matches!(shared.outcome, Outcome::Block));
    }

    // a false alarm verdict lowers the score, and a later fraud verdict removes the entry.
    #[tokio::test]
    async fn a_false_alarm_lowers_the_score_and_fraud_removes_it() {
        // a fresh app and one transaction.
        let app = app();
        // the transaction the case is about.
        decide(&app, &event("e1")).await.unwrap();
        // the analyst says false alarm.
        apply_verdict(&app, &target(), Verdict::FalseAlarm, NOW).await.unwrap();
        // the next transaction carries the allowlist line.
        let cleared = decide(&app, &event("e2")).await.unwrap();
        // check.
        assert!(cleared.reasons.iter().any(|r| r.starts_with("allowlist")));
        // the entry is stored with an end time.
        assert!(app.store.get_user("u1").await.unwrap().allowed_until.is_some());
        // a fraud verdict afterwards removes the entry.
        apply_verdict(&app, &target(), Verdict::Fraud, NOW).await.unwrap();
        // gone.
        assert!(app.store.get_user("u1").await.unwrap().allowed_until.is_none());
    }
}