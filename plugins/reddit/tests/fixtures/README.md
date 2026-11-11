# Reddit plugin fixtures

Every file in this directory is **synthetic**: hand-authored from the documented
Reddit listing schema, because no Reddit credentials are available offline. The
shapes are schema-faithful (`Listing` > `children` > `t3` with `data`), but the
ids, names and timestamps are invented.

Plan 07-12 holds the real-capture checkpoint: it replaces these files with real
responses, using the curl recipes in
`.planning/phases/07-reddit-youtube-plugins/07-RESEARCH.md`. Until then the
research assumptions A1 to A4 stay unconfirmed.

## Convention

- `<name>.json` is the response body.
- An optional `<name>.meta.json` holds
  `{"status": <int>, "headers": {"<lower-case name>": "<value>"}}` for non-200 or
  header-bearing cases. Without it the response is status 200 with no headers.
- Never commit tokens, keys or real account data.

## Files

| File | Content |
|------|---------|
| `listing_new.json` | `GET /r/rust/new`: two self posts and one link post (`is_self` false, external `url`), newest first, with an `after` value |
| `listing_hot.json` | `GET /r/rust/hot`: three posts, one of them `stickied` (the oldest), for the new versus hot sticky rule (D-12) |
| `listing_empty.json` | A `Listing` whose `children` array is empty (an empty page) |
| `listing_removed.json` | A moderator-removed self post (`[removed]`), a deleted-author post (`[deleted]`), a `t1` and a `t5` child, a `t3` child with no `name` and a `t3` child with no `title` |
| `listing_newer.json` | `listing_new` plus two posts that are newer than all of them, placed first: the later-poll case |
| `listing_marked_post_gone.json` | `listing_new` without its newest post: the previously marked post was deleted or removed from the page |

All of these are synthetic (see above); the equal-timestamp, unordered and
tie pages are built inline in `tests/replay.rs` so their timestamps are explicit.
