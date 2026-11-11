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
