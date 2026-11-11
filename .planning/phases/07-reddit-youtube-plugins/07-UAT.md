---
status: testing
phase: 07-reddit-youtube-plugins
source: [07-VERIFICATION.md]
started: 2026-11-11T15:47:50Z
updated: 2026-11-11T15:47:50Z
---

## Current Test

number: 1
name: Reddit app-only live run (A3, A4)
expected: |
  With a real Reddit web-app client id and secret, `fili --registry <registry> plugins run-once reddit` returns real `new` posts from the configured subreddits; no 401/403/404; the request carries the compliant `filimon:reddit:<ver> (by /u/<name>)` User-Agent.
awaiting: user response

## Tests

### 1. Reddit app-only live run (A3, A4)
expected: Real `new` posts returned for the configured subreddits via `plugins run-once reddit`; no 401/403/404; compliant User-Agent.
result: [pending]

### 2. Reddit user feeds live run (A1, A2)
expected: After `fili auth login reddit-user`, `plugins run-once reddit-user` returns items for `feed = "home"` (GET oauth.reddit.com/new) and for `feed = "saved"` with `username` set (/user/<name>/saved, history scope, mixed t3/t1 children).
result: [pending]

### 3. YouTube live run (A11, A13)
expected: With a real YOUTUBE_API_KEY, `plugins run-once youtube` returns recent uploads (UC to UU swap); after `fili auth login youtube-user`, `plugins run-once youtube-user` returns uploads for subscribed channels. Key accepted in the x-goog-api-key header.
result: [pending]

### 4. YouTube quotaExceeded observed (A13)
expected: A real (or inspected) 403 quotaExceeded produces the named message "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific" and the poll backs off, not a silent empty poll.
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
