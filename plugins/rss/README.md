# rss-source

A filimon source plugin that polls RSS 2.0 and Atom 1.0 feeds and returns one
item per entry. It needs no API key and no login.

Syndication feeds are far smaller than the pages they describe, so this is the
cheap way to bring a site's articles into filimon: a news homepage can be
megabytes of HTML, while its feed is tens of kilobytes.

## How it works

1. **The host drives it.** Every `poll_interval_secs`, `fili run` (or
   `fili plugins run-once rss`) calls the plugin's `poll(cursor)`. The plugin has
   no network access of its own: it requests each feed through the host's `fetch`
   import, which enforces the `allowlist` (exact host match), the timeout and TLS
   verification.
2. **It parses the feed.** RSS 2.0 and Atom 1.0 are supported, including
   `content:encoded`, `dc:creator` and `dc:date`. Parsing uses `quick-xml`, which
   never expands custom entities, so a hostile feed cannot mount an
   entity-expansion attack. At most 500 entries are read per feed.
3. **It cleans the content.** Feed text is untrusted. HTML bodies become plain
   text (scripts and styles removed, entities decoded, control characters
   stripped); titles and authors are cleaned the same way. A link must be
   `http(s)`, otherwise the item falls back to the feed URL. Text is cut to
   `max_text_chars`.
4. **It returns only new entries.** The cursor holds one mark per feed: the
   newest publication time seen, plus hashes of the entries published at exactly
   that time, so entries that share a timestamp are neither repeated nor lost.
   The host stores the cursor and hands it back on the next poll. Items are
   returned oldest first.
5. **Items join the normal pipeline.** Each item has `platform = "rss"` and uses
   the entry's guid (or link) as its id, so the host can drop duplicates. The host
   produces items to `filimon-social-items`, and `fili consume` enriches them with
   NER into the same JSON output as every other source.

### Failure handling

| Situation | Result |
|---|---|
| One feed fails, others succeed | A warning (feeds are named by position, never by URL); the others still return and the failing feed keeps its old mark. |
| Every feed fails | A poll error, mapped as below. |
| HTTP 429 | `rate-limited` (honours `Retry-After`). |
| HTTP 408 or 5xx, timeout, network error | `transient`; the host retries with backoff. |
| Host not in `allowlist`, no fetcher assigned, URL refused | `config`; fix the plugin TOML or `filimon.toml`. |
| Other HTTP status, or a document that is not RSS/Atom (for example an HTML error page) | `fatal`. |

## Configuration

Register the source in `filimon.toml` with a fetcher:

```toml
[sources.rss]
component = "plugins/rss/target/wasm32-unknown-unknown/release/rss_source.wasm"
config = "rss.toml"
fetcher = "http"
```

`examples/plugins/rss.toml` is a working plugin TOML. List every feed host in
`host.allowlist`.

| `[settings]` key | Meaning |
|---|---|
| `feeds` | Required. 1-30 `http(s)` feed URLs. |
| `max_items_per_feed` | Newest entries kept per feed per poll. 1-100, default 25. `feeds` times this may not exceed 1000, the host's per-poll item cap. |
| `max_text_chars` | Item text is cut to this many characters. 200-100000, default 20000. |

Unknown keys are rejected, and error messages name the field without repeating
the value.

## Build and run

```bash
cd plugins/rss
cargo component build --release

# from the repository root
fili --registry examples/plugins/filimon.toml plugins check rss
fili --registry examples/plugins/filimon.toml plugins run-once rss            # dry run: prints items
fili --registry examples/plugins/filimon.toml plugins run-once rss --produce  # sends to Fluvio
```

## Limits

- **Excerpt-only feeds give excerpt text.** If a site's feed carries a summary
  rather than the full article (a common WordPress setting), item text is that
  summary. The plugin does not fetch the article page.
- **Undated entries** are returned only on a feed's first poll, because no
  publication mark can order them afterwards.
- **No conditional requests.** The plugin does not send `ETag` or
  `If-Modified-Since`, so each poll downloads the whole feed.
- The cursor is capped at 4096 bytes by the host; the 30-feed limit keeps the
  worst case under it.

## Tests

The core is plain Rust behind an `Http` trait, so the tests replay feeds natively
with no network. A bare `cargo test` targets wasm32 (see `.cargo/config.toml`)
and cannot run; use the host triple:

```bash
cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"
```

## Layout

| File | Role |
|---|---|
| `src/feed.rs` | RSS and Atom parsing into plain entries, date parsing |
| `src/text.rs` | HTML to text, truncation, the hash used for cursor keys |
| `src/settings.rs` | `[settings]` parsing and validation |
| `src/cursor.rs` | The per-feed cursor and its size cap |
| `src/poll.rs` | One poll: fetch, parse, filter by mark, advance the cursor |
| `src/http.rs` | The `Http` trait the core talks to |
| `src/component.rs` | wasm32-only glue to the WIT `fetch` and `log` imports |
