# YouTube plugin fixtures

Every file in this directory is **synthetic**: hand-authored from the documented
YouTube Data API `youtube#playlistItemListResponse` schema, because no Google
credentials are available offline (D-21). The shapes are schema-faithful
(`items` with `snippet` and `contentDetails`, `etag`, `thumbnails`, `position`,
`pageInfo`), but the ids, titles and timestamps are invented.

Plan 07-12 offered the real-capture checkpoint (D-21): the user replied
`synthetic`, so no fixture was replaced and every file below keeps the
`synthetic` label (accepted D-21 fallback; no real capture was made, offered and
declined or unavailable). The research assumptions about the live API (A11, the
`x-goog-api-key` header, and A13, the Google error JSON nesting, among A9 to A13)
therefore remain UNCONFIRMED by a real capture. To replace a fixture later, use
the curl recipe in `.planning/phases/07-reddit-youtube-plugins/07-RESEARCH.md`;
live execution against the real APIs is Phase 8 (TEST-03).

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
| `playlist_newer.json` | `playlist_page.json` plus two newer videos (`vidNew00004`, `vidNew00005`) placed first, as the API lists newest first; used for the later-poll case |
| `playlist_empty.json` | A playlist with no entries: `items` is an empty array |
| `playlist_placeholders.json` | One ordinary upload plus entries titled `Private video` and `Deleted video` and one entry with no `videoId`; the placeholders are newer than the ordinary entry |
| `subscriptions_page.json` | `subscriptions.list` (`mine=true`) response with three subscribed channels (`UCSubscribedChannel00001` to `00003`) and a `nextPageToken`; the synthetic subscription list used by the plan 07-09 tests |

## Provenance of each file

Every file below is **synthetic** (plan 07-12 confirmed no real capture was made).

| Fixture | Provenance | How it was produced |
|---------|------------|---------------------|
| `playlist_page` | synthetic | hand-written from the `playlistItemListResponse` schema |
| `playlist_newer` | synthetic | `playlist_page` plus two newer entries |
| `playlist_empty` | synthetic | schema-shaped response with an empty `items` array |
| `playlist_placeholders` | synthetic | schema-shaped page with `Private video`, `Deleted video` and no-`videoId` entries |
| `quota_exceeded_403` (+ `.meta.json`) | synthetic | Google error envelope, status 403, `errors[0].reason` `quotaExceeded`, domain `youtube.quota` |
| `unauthorized_401` (+ `.meta.json`) | synthetic | Google error envelope, status 401, reason `authError` |
| `malformed` | synthetic | a playlist page cut off mid-string; carries the sentinel word `SENTINELTRUNCATED`, which no error text may echo |
| `subscriptions_page` | synthetic | hand-written `youtube#subscriptionListResponse` (plan 07-09): three entries with valid 24-character `snippet.resourceId.channelId` values, a `nextPageToken` and `pageInfo`; the ids and titles double as the privacy sentinels that no warning or error text may echo |

Other statuses and reasons (429, 400 `keyInvalid`, 403 `accessNotConfigured`,
5xx and so on) are built inline in `tests/replay.rs`.
