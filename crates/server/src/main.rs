// shared pointer so every request handler uses the same store.
use std::sync::Arc;

// web framework pieces: JSON body and response, router, shared state, and the two HTTP methods used.
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
// the decision logic, its settings and its types.
use engine::{AccountEvent, Config, Decision, Engine, TransactionEvent};
// the storage contract and the in memory version.
use store::{InMemoryStore, Store};

// everything the handlers share, the store and the engine.
struct AppState<S> {
    // where user state lives.
    store: S,
    // the rules and scoring.
    engine: Engine,
}

// liveness check, answers with a short text.
async fn health() -> &'static str {
    // the text sent back to the caller.
    "ok"
}

// handles one transaction: read state, decide, save state, return the decision.
async fn evaluate_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the transaction, parsed from the JSON request body.
    Json(event): Json<TransactionEvent>,
) -> Json<Decision> {
    // read the remembered state for this user.
    let state = app.store.get(&event.user_id).await;
    // run the engine, it returns the decision and the updated state.
    let (decision, next) = app.engine.evaluate(&event, &state);
    // save the updated state.
    app.store.put(&event.user_id, next).await;
    // send the decision back as JSON.
    Json(decision)
}

// handles one piece of account activity: read state, record it, save state.
async fn account_event_handler<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the account activity, parsed from the JSON request body.
    Json(event): Json<AccountEvent>,
) -> StatusCode {
    // read the remembered state for this user.
    let mut state = app.store.get(&event.user_id).await;
    // add the activity to the state.
    state.record_account(&event);
    // save the updated state.
    app.store.put(&event.user_id, state).await;
    // answer that the activity was stored and there is nothing to return.
    StatusCode::NO_CONTENT
}

// starts the async runtime and runs the server.
#[tokio::main]
async fn main() {
    // create the store and the engine once and share them through an Arc.
    let app_state = Arc::new(AppState {
        store: InMemoryStore::default(),
        engine: Engine::new(Config::default()),
    });
    // connect each path to its handler and attach the shared store.
    let app = Router::new()
        // GET /health answers the liveness check.
        .route("/health", get(health))
        // POST /v1/evaluate decides one transaction.
        .route("/v1/evaluate", post(evaluate_handler::<InMemoryStore>))
        // POST /v1/account-events records a failed login or a password change.
        .route(
            "/v1/account-events",
            post(account_event_handler::<InMemoryStore>),
        )
        // make the shared state available to every handler.
        .with_state(app_state);
    // listen on localhost only, port 3000.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    // print the address so it is clear where the server runs.
    println!("listening on http://127.0.0.1:3000");
    // serve requests until the process is stopped.
    axum::serve(listener, app).await.unwrap();
}
