// the shared state and the error type of the server.
use crate::{AppState, error::ApiError};
// pieces for a middleware, a function that runs before every handler.
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
// shared pointer to the state.
use std::sync::Arc;
// the storage contract, needed because the state is generic over the store.
use store::Store;

// compares two keys in a time that depends only on their length, not on where they first differ.
fn same_key(a: &[u8], b: &[u8]) -> bool {
    // different lengths can never match, then every byte pair is compared without stopping early.
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

// runs before every protected request, it checks the API key and limits how many requests run at once.
pub async fn guard<S: Store>(
    // the shared application state.
    State(app): State<Arc<AppState<S>>>,
    // the request about to be handled.
    request: Request,
    // the rest of the chain, which ends in the handler.
    next: Next,
) -> Result<Response, ApiError> {
    // the key the caller sent, an empty key when the header is missing.
    let given = request
        .headers()
        .get("x-api-key")
        .map_or(&[][..], |value| value.as_bytes());
    // a wrong key stops here, before the body is read and before any work is done.
    if !same_key(given, app.settings.api_key.as_bytes()) {
        // unauthorized.
        return Err(ApiError::Unauthorized);
    }
    // take one of the places for running requests, when none is free the request is turned away at once.
    let Ok(_permit) = app.permits.try_acquire() else {
        // busy.
        return Err(ApiError::Busy);
    };
    // run the handler, the place is given back when this function ends.
    Ok(next.run(request).await)
}

// tests for the key check.
#[cfg(test)]
mod tests {
    // bring the function above into the tests.
    use super::*;

    // equal keys match, anything else does not.
    #[test]
    fn same_key_matches_only_equal_keys() {
        // equal keys.
        assert!(same_key(b"abcdef", b"abcdef"));
        // one byte different.
        assert!(!same_key(b"abcdef", b"abcdeg"));
        // different lengths.
        assert!(!same_key(b"abcdef", b"abcde"));
        // an empty key never matches a real key.
        assert!(!same_key(b"", b"abcdef"));
    }
}
