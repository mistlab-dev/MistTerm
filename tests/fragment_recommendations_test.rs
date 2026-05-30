//! Unit tests for fragment_recommendations

use mistterm::core::fragment_recommendations::*;

#[test]
fn merge_recommendations_dedup() {
    let a = vec![
        FragmentRecommendation { command: "ls".into(), count: 3, source: "history" },
        FragmentRecommendation { command: "pwd".into(), count: 5, source: "log" },
    ];
    let b = vec![
        FragmentRecommendation { command: "ls".into(), count: 10, source: "log" },
        FragmentRecommendation { command: "whoami".into(), count: 2, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    let ls = merged.iter().find(|r| r.command == "ls").unwrap();
    assert_eq!(ls.count, 10);
    assert!(merged.iter().any(|r| r.command == "pwd"));
    assert!(merged.iter().any(|r| r.command == "whoami"));
}

#[test]
fn merge_recommendations_limit() {
    let a = vec![
        FragmentRecommendation { command: "a".into(), count: 5, source: "history" },
        FragmentRecommendation { command: "b".into(), count: 3, source: "history" },
    ];
    let b = vec![
        FragmentRecommendation { command: "c".into(), count: 10, source: "log" },
    ];
    let merged = merge_recommendations(a, b, 2);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].command, "c");
    assert_eq!(merged[1].command, "a");
}

#[test]
fn merge_recommendations_empty_a() {
    let a: Vec<FragmentRecommendation> = vec![];
    let b = vec![
        FragmentRecommendation { command: "ls".into(), count: 5, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].command, "ls");
}

#[test]
fn merge_recommendations_empty_b() {
    let a = vec![
        FragmentRecommendation { command: "ls".into(), count: 5, source: "history" },
    ];
    let b: Vec<FragmentRecommendation> = vec![];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].command, "ls");
}

#[test]
fn merge_recommendations_both_empty() {
    let a: Vec<FragmentRecommendation> = vec![];
    let b: Vec<FragmentRecommendation> = vec![];

    let merged = merge_recommendations(a, b, 10);
    assert!(merged.is_empty());
}

#[test]
fn merge_recommendations_sort_by_count_descending() {
    let a = vec![
        FragmentRecommendation { command: "low".into(), count: 1, source: "history" },
    ];
    let b = vec![
        FragmentRecommendation { command: "high".into(), count: 100, source: "log" },
        FragmentRecommendation { command: "medium".into(), count: 50, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged[0].command, "high");
    assert_eq!(merged[1].command, "medium");
    assert_eq!(merged[2].command, "low");
}

#[test]
fn fragment_recommendation_debug() {
    let rec = FragmentRecommendation {
        command: "ls".into(),
        count: 5,
        source: "history".into(),
    };
    let debug_str = format!("{:?}", rec);
    assert!(debug_str.contains("ls"));
    assert!(debug_str.contains("5"));
}

#[test]
fn fragment_recommendation_clone() {
    let rec = FragmentRecommendation {
        command: "ls".into(),
        count: 5,
        source: "history".into(),
    };
    let cloned = rec.clone();
    assert_eq!(cloned.command, rec.command);
    assert_eq!(cloned.count, rec.count);
    assert_eq!(cloned.source, rec.source);
}