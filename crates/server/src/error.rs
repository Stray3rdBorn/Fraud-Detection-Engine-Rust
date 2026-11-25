// pieces used to build the answer for a failed request.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
// the error type of the store.
use store::StoreError;

// everything that can make a request fail, each one becomes a status code and a fixed message.
#[derive(Debug)]
pub enum ApiError {
    // the input was wrong, the message names the field and never repeats what the caller sent.
    Invalid(&'static str),
    // the API key is missing or wrong.
    Unauthorized,
    // too many requests are running at once.
    Busy,
    // the store failed or the request took too long.
    Unavailable,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // each kind has a status and a fixed text.
        let (status, message) = match self {
            ApiError::Invalid(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Missing or wrong API key."),
            ApiError::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The service is busy, try again.",
            ),
            ApiError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "No decision could be made, try again.",
            ),
        };
        // the messages are fixed texts without quotes, so they are safe to put into JSON as they are.
        (
            status,
            [("content-type", "application/json")],
            format!("{{\"error\":\"{message}\"}}"),
        )
            .into_response()
    }
}

// a store failure becomes status 503, the payment system decides whether to approve or block meanwhile.
impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        // print the reason for the operator, it never goes to the caller.
        eprintln!("store error: {error}");
        // the caller only learns that no decision was made.
        ApiError::Unavailable
    }
}
