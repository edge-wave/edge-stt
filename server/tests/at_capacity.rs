//! A client past the limit is told where it stands or told to come
//! back. It is never left without an answer.

mod support;

use std::time::Duration;

use edge_stt_server::capacity::{Admission, Capacity};

#[tokio::test]
async fn the_first_through_the_door_starts_at_once() {
    let capacity = Capacity::new(1);
    assert!(matches!(capacity.admit().await, Admission::Started(_)));
}

#[tokio::test]
async fn the_next_one_waits_rather_than_being_dropped() {
    let capacity = Capacity::new(1);
    let holding = capacity.admit().await;
    assert!(matches!(holding, Admission::Started(_)));

    let waiting = tokio::time::timeout(Duration::from_millis(100), capacity.admit()).await;
    assert!(waiting.is_err(), "it should still be waiting, not refused");

    drop(holding);
    assert!(matches!(capacity.admit().await, Admission::Started(_)));
}

#[tokio::test]
async fn a_queued_client_learns_where_it_stands() {
    let capacity = Capacity::new(2);
    let _first = capacity.admit().await;
    let _second = capacity.admit().await;

    let queued = tokio::time::timeout(Duration::from_millis(50), capacity.admit()).await;
    assert!(queued.is_err(), "the third waits behind two");
    assert_eq!(capacity.limit(), 2);
}

#[tokio::test]
async fn a_server_reports_the_limit_it_was_given() {
    let running = support::start(None, 3).await;
    assert_eq!(running.server.capacity.limit(), 3);
}
