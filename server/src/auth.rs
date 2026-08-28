//! The credential the operator configured, checked before any audio is
//! read.

use axum::http::HeaderMap;

pub fn presented(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string)
}

pub fn accepted(expected: Option<&str>, headers: &HeaderMap) -> bool {
    match expected {
        None => true,
        Some(want) => presented(headers).as_deref() == Some(want),
    }
}
