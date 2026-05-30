//! Unit tests for market module
//!
//! Tests serialization of market-related types.

use mistterm::core::market::{MarketCatalogQuery, MarketFragment};

#[test]
fn market_catalog_query_default() {
    let query = MarketCatalogQuery::default();
    assert!(query.category.is_empty());
    assert!(query.search.is_empty());
    assert_eq!(query.limit, 0);
    assert!(query.cursor.is_empty());
}

#[test]
fn market_catalog_query_with_fields() {
    let query = MarketCatalogQuery {
        category: "shell".into(),
        search: "ssh".into(),
        limit: 50,
        cursor: "abc123".into(),
    };
    assert_eq!(query.category, "shell");
    assert_eq!(query.search, "ssh");
    assert_eq!(query.limit, 50);
    assert_eq!(query.cursor, "abc123");
}

#[test]
fn market_fragment_with_data() {
    let fragment = MarketFragment {
        id: "frag-123".into(),
        title: "SSH Connect".into(),
        command: "ssh user@host".into(),
        category: "ssh".into(),
        description: "Connect via SSH".into(),
        author: "MistTerm Team".into(),
        tags: "ssh,connect".into(),
        variables: String::new(),
        revision: 1,
        install_count: 100,
        updated_at: Some("2024-01-01".into()),
    };
    assert_eq!(fragment.id, "frag-123");
    assert_eq!(fragment.title, "SSH Connect");
    assert_eq!(fragment.command, "ssh user@host");
    assert_eq!(fragment.category, "ssh");
    assert_eq!(fragment.description, "Connect via SSH");
    assert_eq!(fragment.author, "MistTerm Team");
    assert_eq!(fragment.tags, "ssh,connect");
    assert_eq!(fragment.install_count, 100);
}

#[test]
fn market_fragment_serde_roundtrip() {
    let fragment = MarketFragment {
        id: "test-id".into(),
        title: "Test Fragment".into(),
        command: "echo hello".into(),
        category: "test".into(),
        description: "A test fragment".into(),
        author: "Test Author".into(),
        tags: "test".into(),
        variables: String::new(),
        revision: 1,
        install_count: 5,
        updated_at: None,
    };

    let json = serde_json::to_string(&fragment).unwrap();
    let deserialized: MarketFragment = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, fragment.id);
    assert_eq!(deserialized.title, fragment.title);
    assert_eq!(deserialized.command, fragment.command);
}

#[test]
fn market_fragment_empty_fields() {
    let fragment = MarketFragment {
        id: String::new(),
        title: String::new(),
        command: String::new(),
        category: String::new(),
        description: String::new(),
        author: String::new(),
        tags: String::new(),
        variables: String::new(),
        revision: 0,
        install_count: 0,
        updated_at: None,
    };

    assert!(fragment.id.is_empty());
    assert!(fragment.title.is_empty());
    assert!(fragment.command.is_empty());
}