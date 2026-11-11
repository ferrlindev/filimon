# API Coverage - Phase 7 (Reddit Data API, YouTube Data API v3)

> Full coverage by default. Opt-outs are explicit, reasoned decisions. Each service starts from its own
> full-coverage baseline: the YouTube matrix re-decides every capability for its own surface and does not
> inherit any Reddit opt-out.

Sources of the opt-out reasons: `07-CONTEXT.md` `<deferred>` (RDDT-04/05 and YTUB-04/05 are v2 in REQUIREMENTS.md),
D-09 (no `videos.list`), D-15 (no `search.list`), D-06 (channel IDs only), D-19 (`username` setting instead of an identity call).

## Reddit

Read endpoints are called from the plugin on `https://oauth.reddit.com` with a bearer token. The token endpoints are
called by the host (`src/plugins/auth`), never by the plugin. Reddit capabilities are prefixed with `reddit` so every
row name is unique across both services.

| capability | decision | reason |
|---|---|---|
| reddit: POST /api/v1/access_token (client_credentials) | INTEGRATE | |
| reddit: GET /api/v1/authorize (authorization code + PKCE) | INTEGRATE | |
| reddit: POST /api/v1/access_token (authorization_code, refresh_token) | INTEGRATE | |
| reddit: POST /api/v1/revoke_token | INTEGRATE | |
| reddit: GET /r/{sub}/new | INTEGRATE | |
| reddit: GET /r/{sub}/hot | INTEGRATE | |
| reddit: GET /new (logged-in home feed) | INTEGRATE | |
| reddit: GET /user/{username}/saved | INTEGRATE | |
| reddit: GET /r/{sub}/top | OPT-OUT | not needed - D-05 limits listings to new and hot |
| reddit: GET /r/{sub}/rising | OPT-OUT | not needed - D-05 limits listings to new and hot |
| reddit: GET /r/{sub}/controversial | OPT-OUT | not needed - D-05 limits listings to new and hot |
| reddit: GET /best | OPT-OUT | not needed - the home feed uses /new, which supports the created_utc high-water mark (D-12) |
| reddit: GET /user/{username}/submitted | OPT-OUT | not needed - no requirement reads another user's posts |
| reddit: GET /user/{username}/upvoted and /hidden | OPT-OUT | not needed - only saved posts are in scope (RDDT-02) |
| reddit: GET /api/v1/me | OPT-OUT | explicitly out of scope - D-19 takes the saved feed username from a setting instead of an identity call and scope |
| reddit: GET /search and /r/{sub}/search | OPT-OUT | deferred to v2 - RDDT-04 (search-query feed) |
| reddit: GET /comments/{article} | OPT-OUT | deferred to v2 - RDDT-05 (post comments) |
| reddit: GET /by_id/{fullnames} and /duplicates/{article} | OPT-OUT | not needed - listings already carry the full post object |
| reddit: GET /subreddits/mine and /r/{sub}/about | OPT-OUT | not needed - subreddits are configured by name, never discovered |
| reddit: GET /api/multi and /r/{sub}/wiki | OPT-OUT | not needed - no requirement covers multireddits or wiki pages |
| reddit: write endpoints (vote, submit, comment, save, subscribe) | OPT-OUT | explicitly out of scope - the plugins are read-only; user instances request read scopes only |
| reddit: GET and POST /message/* | OPT-OUT | explicitly out of scope - private messages are never read or sent |

## YouTube

Calls are made from the plugin on `https://www.googleapis.com/youtube/v3` with an API key header (`youtube` instance)
or a bearer token (`youtube-user` instance). The Google token endpoints are called by the host.

| capability | decision | reason |
|---|---|---|
| youtube: playlistItems.list | INTEGRATE | |
| youtube: subscriptions.list (mine=true) | INTEGRATE | |
| youtube: OAuth authorization code + PKCE (Google auth and token endpoints) | INTEGRATE | |
| youtube: OAuth token revocation (oauth2.googleapis.com/revoke) | INTEGRATE | |
| youtube: search.list | OPT-OUT | explicitly out of scope - 100-unit cost, forbidden by D-15; the search feed is v2 (YTUB-04) |
| youtube: videos.list | OPT-OUT | deferred - D-09 keeps every page at 1 unit; stats and duration are not read |
| youtube: channels.list | OPT-OUT | not needed yet - D-06 takes channel IDs only and derives the uploads playlist without a lookup call; handle resolution is deferred |
| youtube: commentThreads.list and comments.list | OPT-OUT | deferred to v2 - YTUB-05 (video comments) |
| youtube: activities.list | OPT-OUT | not needed - uploads playlists already list new videos at 1 unit per channel |
| youtube: playlists.list | OPT-OUT | not needed - the uploads playlist id is derived from the channel id |
| youtube: channelSections, captions, videoCategories, i18n, guideCategories | OPT-OUT | not needed - no requirement reads them |
| youtube: liveBroadcasts, liveStreams and liveChat endpoints | OPT-OUT | not needed - live streaming is not in scope |
| youtube: insert, update, delete and rate calls on any resource | OPT-OUT | explicitly out of scope - the plugins are read-only; the login scope is youtube.readonly |
| youtube: channelBanners, thumbnails, watermarks and videos upload | OPT-OUT | explicitly out of scope - the plugins never write |
