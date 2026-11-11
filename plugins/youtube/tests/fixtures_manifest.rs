//! The required YouTube fixture cases (D-18, TEST-01). The suite fails if a
//! case is missing, unlabelled or never exercised. Hermetic: it only reads
//! files under the crate directory and this crate's own test source.

use serde_json::Value;
use std::path::PathBuf;

/// (case, fixture name, has a meta file, body is JSON). The self-versus-link
/// post case of D-18 is Reddit-only.
const REQUIRED: &[(&str, &str, bool, bool)] = &[
    ("normal page", "playlist_page", false, true),
    ("empty page", "playlist_empty", false, true),
    (
        "private and deleted placeholders",
        "playlist_placeholders",
        false,
        true,
    ),
    ("403 quotaExceeded", "quota_exceeded_403", true, true),
    ("401 auth failure", "unauthorized_401", true, true),
    ("malformed JSON", "malformed", false, false),
    ("subscriptions page", "subscriptions_page", false, true),
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn every_required_fixture_case_exists_is_labelled_and_is_exercised() {
    let readme = std::fs::read_to_string(fixtures_dir().join("README.md")).expect("README.md");
    let replay = include_str!("replay.rs");

    for (case, name, has_meta, is_json) in REQUIRED {
        let body_path = fixtures_dir().join(format!("{name}.json"));
        let body = std::fs::read(&body_path).unwrap_or_else(|_| panic!("{case}: {name}.json missing"));
        assert!(!body.is_empty(), "{case}: {name}.json is empty");
        if *is_json {
            serde_json::from_slice::<Value>(&body)
                .unwrap_or_else(|_| panic!("{case}: {name}.json is not JSON"));
        }

        if *has_meta {
            let meta_path = fixtures_dir().join(format!("{name}.meta.json"));
            let meta = std::fs::read(&meta_path)
                .unwrap_or_else(|_| panic!("{case}: {name}.meta.json missing"));
            let meta: Value = serde_json::from_slice(&meta)
                .unwrap_or_else(|_| panic!("{case}: {name}.meta.json is not JSON"));
            assert!(meta["status"].is_u64(), "{case}: meta status is not a number");
        }

        let labelled = readme.lines().any(|line| {
            line.contains(name) && (line.contains("synthetic") || line.contains("real"))
        });
        assert!(labelled, "{case}: README does not label {name} synthetic or real");

        assert!(
            replay.contains(&format!("\"{name}\"")),
            "{case}: replay.rs never uses fixture {name}"
        );
    }
}
