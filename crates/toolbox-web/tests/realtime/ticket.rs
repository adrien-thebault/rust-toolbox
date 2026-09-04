use std::{sync::Arc, time::Duration};

use toolbox_auth::Principal;
use toolbox_cluster::InMemoryKvStore;
use toolbox_web::realtime::TicketStore;

fn tickets() -> TicketStore {
    TicketStore::new(Arc::new(InMemoryKvStore::default()))
}

fn principal() -> Principal {
    Principal::new("ada", "password").with_role("ADMIN")
}

#[tokio::test]
async fn a_ticket_round_trips_for_its_own_topic() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();
    let redeemed = tickets.redeem(&ticket, "orders").await.unwrap();
    assert_eq!(redeemed, principal());
}

/// If a ticket leaks into a log it must already be worthless.
#[tokio::test]
async fn a_ticket_is_single_use() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    tickets.redeem(&ticket, "orders").await.unwrap();
    let err = tickets.redeem(&ticket, "orders").await.unwrap_err();
    assert_eq!(err.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(err.problem().code.as_deref(), Some("INVALID_TICKET"));
}

/// The authorization that cannot be skipped: the ticket names its topic.
#[tokio::test]
async fn a_ticket_for_one_topic_cannot_open_another() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    let err = tickets.redeem(&ticket, "payroll").await.unwrap_err();
    assert_eq!(err.status(), http::StatusCode::FORBIDDEN);
    assert_eq!(err.problem().code.as_deref(), Some("TICKET_TOPIC_MISMATCH"));
}

#[tokio::test]
async fn an_unknown_ticket_is_refused() {
    let err = tickets()
        .redeem("not-a-ticket", "orders")
        .await
        .unwrap_err();
    assert_eq!(err.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expired_ticket_is_refused() {
    let tickets = tickets().ttl(Duration::from_millis(20));
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(tickets.redeem(&ticket, "orders").await.is_err());
}

#[tokio::test]
async fn a_ticket_carries_nothing_readable() {
    let ticket = tickets().issue(&principal(), "orders").await.unwrap();
    assert_eq!(ticket.len(), 64, "256 bits, hex");
    assert!(ticket.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!ticket.contains("ada") && !ticket.contains("orders"));
}
