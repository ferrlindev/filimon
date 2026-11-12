// ─────────────────────────────────────────────────────────────────────────────
// fixtures.rs  –  the replay-fetcher's fixture table
//
// This is the only file an author edits. Each entry is `(needle, status, body)`:
// the first entry whose needle is a substring of the request URL answers that
// request with `status` and `body` (bytes, returned unmodified). A needle is
// host plus path (plus whatever query part tells two calls apart) and ignores
// values that vary, such as a `limit` parameter.
//
// Bodies are embedded at build time, so the compiled component carries its
// fixtures and needs no file access at run time.
// ─────────────────────────────────────────────────────────────────────────────

/// `(needle, status, body)`, matched in order against the request URL.
pub const TABLE: &[(&str, u16, &[u8])] = &[
    (
        "oauth.reddit.com/r/rust/new",
        200,
        include_bytes!("../../reddit/tests/fixtures/listing_new.json"),
    ),
    (
        "playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw",
        200,
        include_bytes!("../../youtube/tests/fixtures/playlist_page.json"),
    ),
];
