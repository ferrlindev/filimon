//! TEST-01 / D-18: every required Reddit fixture case exists, is labelled with
//! its provenance in the README, and is exercised by a named test in
//! `replay.rs`. Hermetic: reads only files under this crate, no network, no
//! environment variable (beyond the compile-time manifest dir), no temp files.

use serde_json::Value;
use std::path::PathBuf;

/// (D-18 case, fixture file stem). The `quotaExceeded` case belongs to YouTube.
const REQUIRED: [(&str, &str); 9] = [
    ("normal page and self versus link post", "listing_new"),
    ("empty page", "listing_empty"),
    ("deleted or removed post", "listing_removed"),
    ("429 with rate-limit headers", "rate_limited_429"),
    ("401 auth failure", "unauthorized_401"),
    ("malformed JSON", "malformed"),
    ("hot listing with a stickied post", "listing_hot"),
    ("home feed page (user feed, stickied post)", "home_page"),
    ("saved page (user feed, t3 posts and a t1 comment)", "saved_page"),
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name))
        .unwrap_or_else(|error| panic!("fixture {name} is missing: {error}"))
}

#[test]
fn every_required_fixture_case_exists_is_labelled_and_is_exercised() {
    let readme = String::from_utf8(read("README.md")).expect("README is UTF-8");
    let replay = include_str!("replay.rs");

    for (case, stem) in REQUIRED {
        // The body exists, is non-empty and (except the deliberately broken
        // one) parses as JSON.
        let body_name = format!("{stem}.json");
        let body = read(&body_name);
        assert!(!body.is_empty(), "{case}: {body_name} is empty");
        if stem == "malformed" {
            assert!(
                serde_json::from_slice::<Value>(&body).is_err(),
                "{case}: malformed.json must not parse"
            );
        } else {
            serde_json::from_slice::<Value>(&body)
                .unwrap_or_else(|error| panic!("{case}: {body_name} does not parse: {error}"));
        }

        // A meta file, when present, carries a numeric status.
        let meta_name = format!("{stem}.meta.json");
        if fixtures_dir().join(&meta_name).exists() {
            let meta: Value = serde_json::from_slice(&read(&meta_name))
                .unwrap_or_else(|error| panic!("{case}: {meta_name} does not parse: {error}"));
            assert!(meta["status"].is_u64(), "{case}: {meta_name} needs a numeric status");
        } else {
            assert!(
                !matches!(stem, "rate_limited_429" | "unauthorized_401"),
                "{case}: {meta_name} is required"
            );
        }

        // The README names the file and labels it on the same line.
        let labelled = readme.lines().any(|line| {
            line.contains(stem) && (line.contains("synthetic") || line.contains("real"))
        });
        assert!(labelled, "{case}: README has no `synthetic` or `real` label for {stem}");

        // A named test in replay.rs loads the fixture.
        assert!(
            replay.contains(&format!("\"{stem}\"")),
            "{case}: no test in replay.rs uses the {stem} fixture"
        );
    }
}

#[test]
fn every_fixture_file_in_the_directory_is_named_in_the_readme() {
    let readme = String::from_utf8(read("README.md")).expect("README is UTF-8");
    let mut checked = 0;
    for entry in std::fs::read_dir(fixtures_dir()).expect("fixtures directory") {
        let name = entry.expect("directory entry").file_name();
        let name = name.to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        let stem = stem.strip_suffix(".meta").unwrap_or(stem);
        assert!(
            readme.lines().any(|line| {
                line.contains(stem) && (line.contains("synthetic") || line.contains("real"))
            }),
            "README does not label {name}"
        );
        checked += 1;
    }
    assert!(checked >= 9, "expected the full fixture set, found {checked} files");
}
