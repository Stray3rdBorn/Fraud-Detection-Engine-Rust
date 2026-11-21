// shared pointer so every request handler uses the same store.
use std::sync::Arc;

// web framework pieces: JSON body and response, router, shared state, and the two HTTP methods used.
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
// the decision logic and its types.
use engine::{Decision, TransactionEvent, evaluate};
// the storage contract and the in memory version.
use store::{InMemoryStore, Store};

// liveness check, answers with a short text.
async fn health() -> &'static str {
    // the text sent back to the caller.
    "ok"
}

// handles one transaction: read state, decide, save state, return the decision.
async fn evaluate_handler<S: Store>(
    // the shared store, taken from the application state.
    State(store): State<Arc<S>>,
    // the transaction, parsed from the JSON request body.
    Json(event): Json<TransactionEvent>,
) -> Json<Decision> {
    // read the remembered state for this user.
    let state = store.get(&event.user_id).await;
    // run the engine, it returns the decision and the updated state.
    let (decision, next) = evaluate(&event, &state);
    // save the updated state.
    store.put(&event.user_id, next).await;
    // send the decision back as JSON.
    Json(decision)
}

// starts the async runtime and runs the server.
#[tokio::main]
async fn main() {
    // create the store once and share it through an Arc.
    let store = Arc::new(InMemoryStore::default());
    // connect each path to its handler and attach the shared store.
    let app = Router::new()
        // GET /health answers the liveness check.
        .route("/health", get(health))
        // POST /v1/evaluate decides one transaction.
        .route("/v1/evaluate", post(evaluate_handler::<InMemoryStore>))
        // make the store available to every handler.
        .with_state(store);
    // listen on localhost only, port 3000.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    // print the address so it is clear where the server runs.
    println!("listening on http://127.0.0.1:3000");
    // serve requests until the process is stopped.
    axum::serve(listener, app).await.unwrap();
}