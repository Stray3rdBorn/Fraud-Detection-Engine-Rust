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
use store::{GarnetStore, InMemoryStore, Store};
// the counter that limits how many requests run at once.
use tokio::sync::Semaphore;

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
    // requests that share a user, device or IP wait for each other until the records are saved.
    let _guards = app
        .locks
        .lock(&[&event.user_id, &event.device_id, &event.ip])
        .await;
    // read the user, the device and the IP in one round trip.
    let (state, entities) = app
        .store
        .load(&event.user_id, &event.device_id, &event.ip)
        .await?;
    // run the engine, it returns the decision and the updated records.
    let (decision, next, next_entities) = app.engine.evaluate(event, &state, &entities);
    // save the updated records, all or nothing.
    app.store
        .save(
            &event.user_id,
            &event.device_id,
            &event.ip,
            next,
            next_entities,
        )
        .await?;
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
fn router<S: Store>(store: S, settings: Settings, config: Config) -> Router {
    // the places for running requests, counted before the settings move into the state.
    let permits = Semaphore::new(settings.max_in_flight);
    // the state is created once and shared through an Arc.
    let app_state = Arc::new(AppState {
        store,
        engine: Engine::new(config),
        locks: KeyLocks::new(),
        permits,
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
            )
        }
        // no address means development mode.
        None => {
            // tell the operator which store is in use.
            println!("store: in memory, records are lost when the server stops");
            // build the router around the in memory store.
            router(InMemoryStore::default(), settings, config)
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
