//! A server given several languages serves a client asking for any of them.

use edge_stt_server::Server;

#[test]
fn every_listed_language_is_served_and_no_other() {
    let server = Server::new(None, Some("ko".to_string()), 1, None)
        .with_other_languages(vec!["en".to_string()]);
    assert!(server.serves("ko"));
    assert!(server.serves("en"));
    assert!(!server.serves("ja"));
}

#[test]
fn a_single_language_server_serves_only_that_one() {
    let server = Server::new(None, Some("ko".to_string()), 1, None);
    assert!(server.serves("ko"));
    assert!(!server.serves("en"));
}
