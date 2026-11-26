// the error type of the crate.
use crate::StoreError;
// the transaction and the decision that are stored together.
use engine::{Decision, TransactionEvent};
// the connection pool and its settings.
use sqlx::{PgPool, postgres::PgPoolOptions};
// type for the time limit on connecting.
use serde::Serialize;
use std::time::Duration;

// the tables, created when they do not exist yet.
const SCHEMA: &str = include_str!("schema.sql");
// most connections kept open, the background writer uses one at a time.
const MAX_CONNECTIONS: u32 = 5;
// longest wait for a free connection or for the first connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// one transaction with the decision made for it, the unit that is written to Postgres.
pub struct DecisionRecord {
    // what the payment system sent.
    pub event: TransactionEvent,
    // what the engine answered.
    pub decision: Decision,
}

// the columns of one open case row, in the order of the query.
type CaseColumns = (
    i64,
    String,
    String,
    String,
    String,
    f64,
    Option<String>,
    i64,
    String,
    i32,
    String,
);

// one open case as an analyst sees it, the transaction and the decision together.
#[derive(Debug, Serialize)]
pub struct CaseRow {
    // the number analysts use to refer to the case.
    pub case_id: i64,
    // the transaction the case is about.
    pub event_id: String,
    // the account that made it.
    pub user_id: String,
    // the device it came from.
    pub device_id: String,
    // the IP address it came from.
    pub ip: String,
    // the amount.
    pub amount: f64,
    // the city, when one was sent.
    pub city: Option<String>,
    // the time of the transaction.
    pub event_time: i64,
    // review or block.
    pub outcome: String,
    // the risk score.
    pub score: i32,
    // one line for every rule that fired.
    pub reasons: Vec<String>,
}

// the ids a verdict acts on, taken from the decision behind a case.
pub struct CaseTarget {
    // the transaction, needed as the key of the Garnet write.
    pub event_id: String,
    // the account to block or clear.
    pub user_id: String,
    // the device to block.
    pub device_id: String,
    // the IP address to block.
    pub ip: String,
}

// the Postgres version, it keeps decisions and cases for analysts and audits.
#[derive(Clone)]
pub struct Postgres {
    // connections that many tasks share.
    pool: PgPool,
}

impl Postgres {
    // connects to Postgres and makes sure the tables exist, the url looks like postgres://user:password@127.0.0.1:5432/fraud.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        // open the pool, a wrong address or password fails here.
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(CONNECT_TIMEOUT)
            .connect(url)
            .await?;
        // create the tables, running it twice changes nothing.
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        // keep the pool.
        Ok(Postgres { pool })
    }

    // writes many decisions in one statement, and opens a case for every one that is not allow.
    pub async fn insert_decisions(&self, batch: &[DecisionRecord]) -> Result<(), StoreError> {
        // each column of the batch as its own list, Postgres turns the lists back into rows.
        let event_ids: Vec<&str> = batch.iter().map(|r| r.event.event_id.as_str()).collect();
        // the users.
        let user_ids: Vec<&str> = batch.iter().map(|r| r.event.user_id.as_str()).collect();
        // the devices.
        let device_ids: Vec<&str> = batch.iter().map(|r| r.event.device_id.as_str()).collect();
        // the IP addresses.
        let ips: Vec<&str> = batch.iter().map(|r| r.event.ip.as_str()).collect();
        // the amounts.
        let amounts: Vec<f64> = batch.iter().map(|r| r.event.amount).collect();
        // the cities, some are empty.
        let cities: Vec<Option<&str>> = batch.iter().map(|r| r.event.city.as_deref()).collect();
        // the transaction times.
        let times: Vec<i64> = batch.iter().map(|r| r.event.timestamp).collect();
        // the outcomes as words.
        let outcomes: Vec<&str> = batch.iter().map(|r| r.decision.outcome.as_str()).collect();
        // the scores.
        let scores: Vec<i32> = batch.iter().map(|r| r.decision.score as i32).collect();
        // the reasons of each decision as one JSON text, the database turns it into a JSON value.
        let reasons = batch
            .iter()
            .map(|r| serde_json::to_string(&r.decision.reasons))
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
        // one statement does both inserts, so a decision and its case are saved together or not at all.
        sqlx::query(
            "WITH new AS (
                 INSERT INTO decisions
                     (event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons)
                 SELECT event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons::jsonb
                 FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::float8[],
                             $6::text[], $7::int8[], $8::text[], $9::int4[], $10::text[])
                     AS t(event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons)
                 ON CONFLICT (event_id) DO NOTHING
                 RETURNING event_id, outcome
             )
             INSERT INTO cases (event_id)
             SELECT event_id FROM new WHERE outcome <> 'allow'
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(event_ids)
        .bind(user_ids)
        .bind(device_ids)
        .bind(ips)
        .bind(amounts)
        .bind(cities)
        .bind(times)
        .bind(outcomes)
        .bind(scores)
        .bind(reasons)
        .execute(&self.pool)
        .await?;
        // done.
        Ok(())
    }

    // lists open cases oldest first, only those after the given case number, so an analyst can page through them.
    pub async fn open_cases(&self, after: i64, limit: i64) -> Result<Vec<CaseRow>, StoreError> {
        // one row per open case with its decision, the reasons come back as text and are parsed below.
        let rows: Vec<CaseColumns> = sqlx::query_as(
            "SELECT c.case_id, d.event_id, d.user_id, d.device_id, d.ip, d.amount, d.city,
                    d.event_time, d.outcome, d.score, d.reasons::text
             FROM cases c JOIN decisions d USING (event_id)
             WHERE c.status = 'open' AND c.case_id > $1
             ORDER BY c.case_id
             LIMIT $2",
        )
        .bind(after)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        // turn each row into a case.
        rows.into_iter()
            .map(
                |(
                    case_id,
                    event_id,
                    user_id,
                    device_id,
                    ip,
                    amount,
                    city,
                    event_time,
                    outcome,
                    score,
                    reasons,
                )| {
                    // the reasons were written by this program, so a parse failure means a damaged row.
                    let reasons = serde_json::from_str(&reasons)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
                    // the finished case.
                    Ok(CaseRow {
                        case_id,
                        event_id,
                        user_id,
                        device_id,
                        ip,
                        amount,
                        city,
                        event_time,
                        outcome,
                        score,
                        reasons,
                    })
                },
            )
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(StoreError::from)
    }

    // finds the ids behind an open case, nothing comes back for a missing or closed case.
    pub async fn case_target(&self, case_id: i64) -> Result<Option<CaseTarget>, StoreError> {
        // one row at most.
        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT d.event_id, d.user_id, d.device_id, d.ip
             FROM cases c JOIN decisions d USING (event_id)
             WHERE c.case_id = $1 AND c.status = 'open'",
        )
        .bind(case_id)
        .fetch_optional(&self.pool)
        .await?;
        // name the columns.
        Ok(row.map(|(event_id, user_id, device_id, ip)| CaseTarget {
            event_id,
            user_id,
            device_id,
            ip,
        }))
    }

    // closes an open case and stores the verdict in one statement, false means the case was missing or already closed.
    pub async fn close_case(
        &self,
        case_id: i64,
        verdict: &str,
        note: Option<&str>,
    ) -> Result<bool, StoreError> {
        // the update and the insert share one statement, so a case is never closed without its verdict.
        let done = sqlx::query(
            "WITH closed AS (
                 UPDATE cases SET status = 'closed'
                 WHERE case_id = $1 AND status = 'open'
                 RETURNING case_id
             )
             INSERT INTO verdicts (case_id, verdict, note)
             SELECT case_id, $2, $3 FROM closed",
        )
        .bind(case_id)
        .bind(verdict)
        .bind(note)
        .execute(&self.pool)
        .await?;
        // one inserted row means this call closed the case.
        Ok(done.rows_affected() == 1)
    }
}
