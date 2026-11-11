---
status: complete
phase: 07-reddit-youtube-plugins
source: [07-VERIFICATION.md]
started: 2026-11-11T15:47:50Z
updated: 2026-11-11T16:13:49.683Z
---

## Current Test

[testing complete]

## Tests

### 1. Reddit app-only live run (A3, A4)
expected: Real `new` posts returned for the configured subreddits via `plugins run-once reddit`; no 401/403/404; compliant User-Agent.
result: pass

### 2. Reddit user feeds live run (A1, A2)
expected: After `fili auth login reddit-user`, `plugins run-once reddit-user` returns items for `feed = "home"` (GET oauth.reddit.com/new) and for `feed = "saved"` with `username` set (/user/<name>/saved, history scope, mixed t3/t1 children).
result: pass

### 3. YouTube live run (A11, A13)
expected: With a real YOUTUBE_API_KEY, `plugins run-once youtube` returns recent uploads (UC to UU swap); after `fili auth login youtube-user`, `plugins run-once youtube-user` returns uploads for subscribed channels. Key accepted in the x-goog-api-key header.
result: pass

### 4. YouTube quotaExceeded observed (A13)
expected: A real (or inspected) 403 quotaExceeded produces the named message "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific" and the poll backs off, not a silent empty poll.
result: pass

## Summary

total: 4
passed: 4
issues: 0
pending: 0
skipped: 0
blocked: 0

## Gaps
