use super::{build_index_from_jsonl, fuzz_search, regex_search, Index};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn write_jsonl(path: &Path, lines: &[&str]) {
    let mut file = File::create(path).expect("create jsonl");
    for line in lines {
        writeln!(file, "{}", line).expect("write jsonl");
    }
}

fn temp_path(name: &str, ext: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis();
    path.push(format!("minindex_test_{}_{}.{}", name, stamp, ext));
    path
}

#[test]
fn build_index_and_fuzz_search() {
    let jsonl_path = temp_path("build", "jsonl");
    let idx_path = temp_path("build", "idx");

    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"Getting Started\",\"body\":\"This guide will help you get started.\",\"category\":\"docs\",\"href\":\"/docs/getting-started\"}",
            "{\"title\":\"API Reference\",\"body\":\"Complete API documentation for search functions.\",\"category\":\"reference\",\"href\":\"/docs/api\"}",
        ],
    );

    let fields = vec![
        "title".to_string(),
        "body".to_string(),
        "category".to_string(),
        "href".to_string(),
    ];
    let index = build_index_from_jsonl(&jsonl_path, &fields, Some("title"), None)
        .expect("build index");
    index.to_path(&idx_path).expect("write index");

    let reloaded = Index::from_path(&idx_path).expect("read index");
    assert_eq!(reloaded.doc_count(), 2);

    let results = fuzz_search(&reloaded, "getting started", 5).expect("search");
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "Getting Started");
    assert_eq!(
        results[0].document.get("title").map(String::as_str),
        Some("Getting Started")
    );
}

#[test]
fn fuzz_search_empty_query() {
    let jsonl_path = temp_path("empty_query", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"Doc\",\"body\":\"Body\",\"category\":\"docs\",\"href\":\"/doc\"}",
        ],
    );

    let fields = vec![
        "title".to_string(),
        "body".to_string(),
        "category".to_string(),
        "href".to_string(),
    ];
    let index = build_index_from_jsonl(&jsonl_path, &fields, None, None).expect("build index");
    let results = fuzz_search(&index, "   ", 5).expect("search");
    assert!(results.is_empty());
}

#[test]
fn regex_search_matches() {
    let jsonl_path = temp_path("regex", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"API Reference\",\"body\":\"API docs for core search.\",\"category\":\"reference\",\"href\":\"/docs/api\"}",
            "{\"title\":\"Guide\",\"body\":\"Learn Rust search.\",\"category\":\"docs\",\"href\":\"/docs/guide\"}",
        ],
    );

    let fields = vec![
        "title".to_string(),
        "body".to_string(),
        "category".to_string(),
        "href".to_string(),
    ];
    let index = build_index_from_jsonl(&jsonl_path, &fields, None, None).expect("build index");
    let results = regex_search(&index, "API", 5).expect("regex search");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, "API Reference");
    assert_eq!(results[0].matches[0], "API");
}

#[test]
fn missing_field_errors() {
    let jsonl_path = temp_path("missing_field", "jsonl");
    write_jsonl(
        &jsonl_path,
        &["{\"title\":\"Doc\",\"body\":\"Body\",\"href\":\"/doc\"}"],
    );

    let fields = vec![
        "title".to_string(),
        "body".to_string(),
        "category".to_string(),
    ];
    let err = build_index_from_jsonl(&jsonl_path, &fields, None, None).unwrap_err();
    assert!(err.to_string().contains("Missing required field"));
}

#[test]
fn more_fields_are_returned_when_present() {
    let jsonl_path = temp_path("extra_fields", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"Extra\",\"body\":\"Has extra metadata.\",\"category\":\"docs\",\"href\":\"/docs/extra\"}",
            "{\"title\":\"Plain\",\"body\":\"No extras here.\"}",
        ],
    );

    let fields = vec!["body".to_string()];
    let more_fields = vec!["category".to_string(), "href".to_string()];
    let index =
        build_index_from_jsonl(&jsonl_path, &fields, Some("title"), Some(&more_fields))
            .expect("build index");
    assert_eq!(index.doc_count(), 2);

    let bytes = index.to_bytes().expect("serialize bytes");
    let reloaded = Index::from_bytes(&bytes).expect("deserialize bytes");

    let results = fuzz_search(&reloaded, "extra metadata", 5).expect("search");
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "Extra");
    assert_eq!(
        results[0].document.get("body").map(String::as_str),
        Some("Has extra metadata.")
    );
    assert_eq!(
        results[0].document.get("title").map(String::as_str),
        Some("Extra")
    );
    assert_eq!(
        results[0].document.get("category").map(String::as_str),
        Some("docs")
    );
    assert_eq!(
        results[0].document.get("href").map(String::as_str),
        Some("/docs/extra")
    );
}

#[test]
fn fuzz_search_terms_are_anded() {
    let jsonl_path = temp_path("and_terms", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"Alpha Beta\",\"body\":\"alpha beta gamma\"}",
            "{\"title\":\"Alpha Only\",\"body\":\"alpha\"}",
            "{\"title\":\"Beta Only\",\"body\":\"beta\"}",
        ],
    );

    let fields = vec!["title".to_string(), "body".to_string()];
    let index = build_index_from_jsonl(&jsonl_path, &fields, Some("title"), None)
        .expect("build index");
    let results = fuzz_search(&index, "alpha beta", 10).expect("search");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, "Alpha Beta");
}

#[test]
fn fuzz_search_ignores_stop_words() {
    let jsonl_path = temp_path("stop_words", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"Getting Started\",\"body\":\"This guide will help you get started.\"}",
            "{\"title\":\"Other\",\"body\":\"Completely different.\"}",
        ],
    );

    let fields = vec!["title".to_string(), "body".to_string()];
    let index = build_index_from_jsonl(&jsonl_path, &fields, Some("title"), None)
        .expect("build index");
    let results = fuzz_search(&index, "getting started", 10).expect("search");
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "Getting Started");
}

#[test]
fn serialize_roundtrip_bytes() {
    let jsonl_path = temp_path("roundtrip_bytes", "jsonl");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"First\",\"body\":\"alpha beta\",\"category\":\"docs\"}",
            "{\"title\":\"Second\",\"body\":\"beta gamma\",\"category\":\"docs\"}",
        ],
    );

    let fields = vec!["title".to_string(), "body".to_string(), "category".to_string()];
    let index = build_index_from_jsonl(&jsonl_path, &fields, Some("title"), None)
        .expect("build index");
    let bytes = index.to_bytes().expect("serialize bytes");
    let reloaded = Index::from_bytes(&bytes).expect("deserialize bytes");

    assert_eq!(reloaded.doc_count(), 2);
    let results = fuzz_search(&reloaded, "alpha", 5).expect("search");
    assert_eq!(results[0].doc_id, "First");
}

#[test]
fn serialize_roundtrip_path() {
    let jsonl_path = temp_path("roundtrip_path", "jsonl");
    let idx_path = temp_path("roundtrip_path", "idx");
    write_jsonl(
        &jsonl_path,
        &[
            "{\"title\":\"One\",\"body\":\"foo bar\",\"category\":\"docs\"}",
            "{\"title\":\"Two\",\"body\":\"baz qux\",\"category\":\"docs\"}",
        ],
    );

    let fields = vec!["title".to_string(), "body".to_string(), "category".to_string()];
    let index = build_index_from_jsonl(&jsonl_path, &fields, Some("title"), None)
        .expect("build index");
    index.to_path(&idx_path).expect("write index");

    let reloaded = Index::from_path(&idx_path).expect("read index");
    assert_eq!(reloaded.doc_count(), 2);
    let results = regex_search(&reloaded, "foo", 5).expect("regex search");
    assert_eq!(results[0].doc_id, "One");
}
