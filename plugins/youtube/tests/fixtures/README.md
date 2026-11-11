# YouTube plugin fixtures

Every file in this directory is **synthetic**: hand-authored from the documented
YouTube Data API `youtube#playlistItemListResponse` schema, because no Google
credentials are available offline (D-21). The shapes are schema-faithful
(`items` with `snippet` and `contentDetails`, `etag`, `thumbnails`, `position`,
`pageInfo`), but the ids, titles and timestamps are invented.

Plan 07-12 holds the real-capture checkpoint: it replaces these files with real
responses, using the curl recipe in
`.planning/phases/07-reddit-youtube-plugins/07-RESEARCH.md`. Until then the
research assumptions about the live API (A9 to A13) stay unconfirmed.

## Convention

- `<name>.json` is the response body.
- An optional `<name>.meta.json` holds
  `{"status": <int>, "headers": {"<lower-case name>": "<value>"}}` for non-200 or
  header-bearing cases. Without it the response is status 200 with no headers.
- Never commit API keys, tokens or real account data.

## Files

| File | Content |
|------|---------|
| `playlist_page.json` | `playlistItems.list` for `UU_x5XG1OV2P6uZZ5FSM9Ttw`: three videos, newest first. The first has a `contentDetails.videoPublishedAt` that differs from its `snippet.publishedAt`, the second has no `videoPublishedAt` and no `videoOwnerChannelTitle`, the third is ordinary |
