// shared pointer so every request handler uses the same store.
use std::sync::Arc;
// future type and time helpers.
use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// web framework pieces: JSON body and response, router, shared state, body size limit, middleware, status codes and the two HTTP methods used.
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
// the decision logic, its settings and its types.
use engine::{AccountEvent, Config, Decision, Engine, TransactionEvent};
// the answers for failed requests.
use error::ApiError;
// the locks that keep requests for the same user, device or IP in line.
use locks::KeyLocks;
// the settings read at startup.
use settings::Settings;
// the storage contract and both versions.
use store::{DecisionRecord, GarnetStore, InMemoryStore, Keys, Postgres, Store};
// the counter that limits how many requests run at once.
use tokio::sync::Semaphore;
// the queue that feeds the decision log.
use recorder::Recorder;

// the API key check and the limit on running requests.
mod auth;
// the answers for failed requests.
mod error;
// locks per user, device and IP.
mod locks;
// the settings read at startup.
mod settings;
// the checks on incoming data.
mod validate;
// the queue and the task that write decisions to Postgres.
mod recorder;

// largest request body accepted, in bytes, a real transaction is well under one kilobyte.
const MAX_BODY_BYTES: usize = 4096;

// everything the handlers share, the store, the engine, the locks and the limits.
struct AppState<S> {
    // where records live.
    store: S,
    // the rules and scoring.
    engine: Engine,
    // keeps read then write sequences of different requests apart.
    locks: KeyLocks,
    // the places for running requests, one is taken per request.
    permits: Semaphore,
    // the decision log, empty when no Postgres address is set.
    recorder: Option<Recorder>,
    // the startup settings, including the API key.
    settings: Settings,
}

// the current time in unix seconds, zero when the clock is before 1970, which makes every time check fail.
fn unix_now() -> i64 {
    // seconds since 1970.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

// runs the work, and gives up with an error when it takes longer than the limit, the locks are freed by that.
async fn within<T>(
    limit: Duration,
    work: impl Future<Output = Result<T, ApiError>>,
) -> Result<T, ApiError> {
    // a timeout becomes the same error as a store failure.
    tokio::time::timeout(limit, work)
        .await
        .unwrap_or(Err(ApiError::Unavailable))
}

// liveness check, answers with a short text, it needs no key.
async fn health() -> &'static str {
    // the text sent back to the caller.
    "ok"
}

// does the work of one decision: lock, read records, decide, save records.
async fn decide<S: Store>(
    app: &AppState<S>,
    event: &TransactionEvent,
) -> Result<Decision, ApiError> {
    // requests that share a user, device, IP or event id wait for each other until the records are saved.
    let _guards = app
        .locks
        .lock(&[&event.user_id, &event.device_id, &event.ip, &event.event_id])
        .await;
    // the ids that tell the store what to read and write.
    let keys = Keys {
        user_id: &event.user_id,
        device_id: &event.device_id,
        ip: &event.ip,
        event_id: &event.event_id,
    };
    // read the user, the device, the IP and any earlier decision for this event in one round trip.
    let (state, entities, earlier) = app.store.load(&keys).await?;
    // a repeated event id gets the first decision again, nothing is counted twice.
    if let Some(decision) = earlier {
        // the answer from the first time.
        return Ok(decision);
    }
    // run the engine, it returns the decision and the updated records.
    let (decision, next, next_entities) = app.engine.evaluate(event, &state, &entities);
    // save the updated records and the decision, all or nothing.
    app.store
        .save(&keys, next, next_entities, &decision)
        .await?;
    // queue the decision for the log, only after it is saved, so a failed request leaves no entry.
    if let Some(recorder) = &app.recorder {
        // the transaction and its decision are written together.
        recorder.record(DecisionRecord {
            event: event.clone(),
            decision: decision.clone(),
        });
    }
    // hand back the decision.
    Ok(decision)
}

// handles one transaction: check the input, decide, return the decision.
async fn evaluate_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the transaction, parsed from the JSON request body.
    Json(mut event): Json<TransactionEvent>,
) -> Result<Json<Decision>, ApiError> {
    // refuse bad input before any work is done.
    validate::transaction(&mut event, unix_now(), app.settings.max_clock_skew)?;
    // decide within the time limit and send the decision back as JSON.
    Ok(Json(
        within(app.settings.request_timeout, decide(&app, &event)).await?,
    ))
}

// does the work of one account event: lock, read state, record it, save state.
async fn record_account<S: Store>(app: &AppState<S>, event: &AccountEvent) -> Result<(), ApiError> {
    // requests for the same user wait for each other until the state is saved.
    let _guards = app.locks.lock(&[&event.user_id]).await;
    // read the remembered state for this user.
    let mut state = app.store.get_user(&event.user_id).await?;
    // add the activity to the state.
    state.record_account(event);
    // save the updated state.
    app.store.put_user(&event.user_id, state).await?;
    // done.
    Ok(())
}

// handles one piece of account activity: check the input, record it.
async fn account_event_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the account activity, parsed from the JSON request body.
    Json(event): Json<AccountEvent>,
) -> Result<StatusCode, ApiError> {
    // refuse bad input before any work is done.
    validate::account(&event, unix_now(), app.settings.max_clock_skew)?;
    // record within the time limit.
    within(app.settings.request_timeout, record_account(&app, &event)).await?;
    // answer that the activity was stored and there is nothing to return.
    Ok(StatusCode::NO_CONTENT)
}

// builds the router for any store version.
fn router<S: Store>(
    store: S,
    settings: Settings,
    config: Config,
    recorder: Option<Recorder>,
) -> Router {
    // the places for running requests, counted before the settings move into the state.
    let permits = Semaphore::new(settings.max_in_flight);
    // the state is created once and shared through an Arc.
    let app_state = Arc::new(AppState {
        store,
        engine: Engine::new(config),
        locks: KeyLocks::new(),
        permits,
        recorder,
        settings,
    });
    // the routes that need the API key.
    let protected = Router::new()
        // POST /v1/evaluate decides one transaction.
        .route("/v1/evaluate", post(evaluate_handler::<S>))
        // POST /v1/account-events records a failed login or a password change.
        .route("/v1/account-events", post(account_event_handler::<S>))
        // bodies above the limit are refused with status 413.
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        // the key check runs first, so a wrong key never reaches the body or the handler.
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            auth::guard::<S>,
        ))
        // make the shared state available to every handler.
        .with_state(app_state);
    // GET /health stays open, everything else needs the key.
    Router::new().route("/health", get(health)).merge(protected)
}

// prints the reason and stops, used when the startup settings are wrong.
fn fail(message: &str) -> ! {
    // tell the operator what to fix.
    eprintln!("startup error: {message}");
    // stop with an error code.
    std::process::exit(1);
}

// starts the async runtime and runs the server.
#[tokio::main]
async fn main() {
    // read the startup settings, a missing or weak key stops the server.
    let settings = Settings::from_env().unwrap_or_else(|error| fail(&error));
    // read the rule settings, a bad file stops the server.
    let config = settings::load_engine_config(settings.engine_config.as_deref())
        .unwrap_or_else(|error| fail(&error));
    // tell the operator where the rule settings came from.
    match &settings.engine_config {
        // a file.
        Some(path) => println!("rule settings: {path}"),
        // the built in defaults.
        None => println!("rule settings: defaults"),
    }
    // the decision log, connect once at startup, a wrong address or password stops the server right away.
    let recorder = match &settings.database_url {
        // the address can hold a password and is never printed.
        Some(url) => {
            // tell the operator.
            println!("decision log: Postgres");
            // connect, create the tables and start the writing task.
            Some(Recorder::start(
                Postgres::connect(url).await.unwrap_or_else(|error| {
                    fail(&format!("could not connect to Postgres: {error}"))
                }),
            ))
        }
        // no address means no log.
        None => {
            // tell the operator.
            println!("decision log: off, decisions and cases are not stored");
            // nothing to feed.
            None
        }
    };
    // the address and the store address are needed after the settings move into the router.
    let bind = settings.bind;
    // the Garnet address can hold a password and is never printed.
    let garnet_url = settings.garnet_url.clone();
    // pick the store.
    let app = match garnet_url {
        // connect once at startup, a wrong address or password stops the server right away.
        Some(url) => {
            // tell the operator which store is in use.
            println!("store: Garnet");
            // build the router around the Garnet store.
            router(
                GarnetStore::connect(&url)
                    .await
                    .unwrap_or_else(|error| fail(&format!("could not connect to Garnet: {error}"))),
                settings,
                config,
                recorder,
            )
        }
        // no address means development mode.
        None => {
            // tell the operator which store is in use.
            println!("store: in memory, records are lost when the server stops");
            // build the router around the in memory store.
            router(InMemoryStore::default(), settings, config, recorder)
        }
    };
    // listen on the configured address.
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|error| fail(&format!("could not listen on {bind}: {error}")));
    // print the address so it is clear where the server runs.
    println!("listening on http://{bind}");
    // serve requests until the process is stopped.
    axum::serve(listener, app).await.unwrap();
}

// tests for the decision flow.
#[cfg(test)]
mod tests {
    // bring the flow and its types into the tests.
    use super::*;

    // an app with the in memory store and no decision log.
    fn app() -> AppState<InMemoryStore> {
        // every field is filled, the settings values do not matter for these tests.
        AppState {
            store: InMemoryStore::default(),
            engine: Engine::new(Config::default()),
            locks: KeyLocks::new(),
            permits: Semaphore::new(1),
            recorder: None,
            settings: Settings {
                api_key: "k".repeat(16),
                bind: "127.0.0.1:0".parse().unwrap(),
                garnet_url: None,
                database_url: None,
                engine_config: None,
                max_clock_skew: None,
                request_timeout: Duration::from_secs(2),
                max_in_flight: 1,
            },
        }
    }

    // a transaction with the given event id.
    fn event(event_id: &str) -> TransactionEvent {
        // every field filled with a normal value.
        TransactionEvent {
            event_id: event_id.to_string(),
            user_id: "u1".to_string(),
            timestamp: 1_700_000_000,
            amount: 25.0,
            device_id: "d1".to_string(),
            ip: "1.2.3.4".to_string(),
            signup_time: None,
            city: None,
        }
    }

    // a repeated event id gets the first decision back and counts only once.
    #[tokio::test]
    async fn a_repeated_event_id_gets_the_same_decision_and_counts_once() {
        // a fresh app.
        let app = app();
        // the first request.
        let first = decide(&app, &event("e1")).await.unwrap();
        // the same event again, as a retry would send it.
        let second = decide(&app, &event("e1")).await.unwrap();
        // the same answer.
        assert_eq!(first, second);
        // the user's counter saw one transaction, not two.
        let state = app.store.get_user("u1").await.unwrap();
        // check.
        assert_eq!(state.velocity.count(1_700_000_000), 1);
        // a different event id is a new transaction and counts.
        decide(&app, &event("e2")).await.unwrap();
        // the counter now shows two.
        let state = app.store.get_user("u1").await.unwrap();
        // check.
        assert_eq!(state.velocity.count(1_700_000_000), 2);
    }
}
