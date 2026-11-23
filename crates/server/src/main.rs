// shared pointer so every request handler uses the same store.
use std::sync::Arc;

// web framework pieces: JSON body and response, router, shared state, status codes, and the two HTTP methods used.
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
// the decision logic, its settings and its types.
use engine::{AccountEvent, Config, Decision, Engine, TransactionEvent};
// the locks that keep requests for the same user, device or IP in line.
use locks::KeyLocks;
// the storage contract, both versions and the error type.
use store::{GarnetStore, InMemoryStore, Store, StoreError};

// locks per user, device and IP.
mod locks;

// everything the handlers share, the store, the engine and the locks.
struct AppState<S> {
    // where records live.
    store: S,
    // the rules and scoring.
    engine: Engine,
    // keeps read then write sequences of different requests apart.
    locks: KeyLocks,
}

// liveness check, answers with a short text.
async fn health() -> &'static str {
    // the text sent back to the caller.
    "ok"
}

// turns a store failure into status 503, the payment system decides whether to approve or block while no decision is possible.
fn store_unavailable(error: StoreError) -> StatusCode {
    // print the reason for the operator, it never goes to the caller.
    eprintln!("store error: {error}");
    // service unavailable.
    StatusCode::SERVICE_UNAVAILABLE
}

// handles one transaction: read records, decide, save records, return the decision.
async fn evaluate_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the transaction, parsed from the JSON request body.
    Json(event): Json<TransactionEvent>,
) -> Result<Json<Decision>, StatusCode> {
    // requests that share a user, device or IP wait for each other until the records are saved.
    let _guards = app
        .locks
        .lock(&[&event.user_id, &event.device_id, &event.ip])
        .await;
    // read the user, the device and the IP in one round trip.
    let (state, entities) = app
        .store
        .load(&event.user_id, &event.device_id, &event.ip)
        .await
        .map_err(store_unavailable)?;
    // run the engine, it returns the decision and the updated records.
    let (decision, next, next_entities) = app.engine.evaluate(&event, &state, &entities);
    // save the updated records, all or nothing.
    app.store
        .save(
            &event.user_id,
            &event.device_id,
            &event.ip,
            next,
            next_entities,
        )
        .await
        .map_err(store_unavailable)?;
    // send the decision back as JSON.
    Ok(Json(decision))
}

// handles one piece of account activity: read state, record it, save state.
async fn account_event_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the account activity, parsed from the JSON request body.
    Json(event): Json<AccountEvent>,
) -> Result<StatusCode, StatusCode> {
    // requests for the same user wait for each other until the state is saved.
    let _guards = app.locks.lock(&[&event.user_id]).await;
    // read the remembered state for this user.
    let mut state = app
        .store
        .get_user(&event.user_id)
        .await
        .map_err(store_unavailable)?;
    // add the activity to the state.
    state.record_account(&event);
    // save the updated state.
    app.store
        .put_user(&event.user_id, state)
        .await
        .map_err(store_unavailable)?;
    // answer that the activity was stored and there is nothing to return.
    Ok(StatusCode::NO_CONTENT)
}

// builds the router for any store version.
fn router<S: Store>(store: S) -> Router {
    // the state is created once and shared through an Arc.
    let app_state = Arc::new(AppState {
        store,
        engine: Engine::new(Config::default()),
        locks: KeyLocks::new(),
    });
    // connect each path to its handler and attach the shared state.
    Router::new()
        // GET /health answers the liveness check.
        .route("/health", get(health))
        // POST /v1/evaluate decides one transaction.
        .route("/v1/evaluate", post(evaluate_handler::<S>))
        // POST /v1/account-events records a failed login or a password change.
        .route("/v1/account-events", post(account_event_handler::<S>))
        // make the shared state available to every handler.
        .with_state(app_state)
}

// starts the async runtime and runs the server.
#[tokio::main]
async fn main() {
    // the Garnet address comes from the environment because it can hold a password, without it the in memory store is used.
    let app = match std::env::var("GARNET_URL") {
        // connect once at startup, a wrong address or password stops the server right away.
        Ok(url) => {
            // tell the operator which store is in use.
            println!("store: Garnet");
            // build the router around the Garnet store.
            router(
                GarnetStore::connect(&url)
                    .await
                    .expect("could not connect to Garnet"),
            )
        }
        // no address means development mode.
        Err(_) => {
            // tell the operator which store is in use.
            println!("store: in memory, records are lost when the server stops");
            // build the router around the in memory store.
            router(InMemoryStore::default())
        }
    };
    // listen on localhost only, port 3000.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    // print the address so it is clear where the server runs.
    println!("listening on http://127.0.0.1:3000");
    // serve requests until the process is stopped.
    axum::serve(listener, app).await.unwrap();
}