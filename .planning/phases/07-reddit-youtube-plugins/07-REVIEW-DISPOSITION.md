---
phase: 07
review: 07-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "Rate-limit handling aborts the whole poll, discards good pages, and can starve later feeds"
  - id: WR-02
    severity: warning
    disposition: open
    title: "One failing feed disables the entire plugin (or wedges it) instead of skipping the feed"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`order=relevance` subscription window is unstable, so marks are dropped and videos re-emitted"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Reddit time-mark cursor stores an unbounded API-supplied fullname"
  - id: WR-05
    severity: warning
    disposition: open
    title: "Cached app-only token is never invalidated when the API rejects it"
  - id: WR-06
    severity: warning
    disposition: open
    title: "`x-goog-api-key` is not dropped on a cross-origin redirect"
  - id: IN-01
    severity: info
    disposition: open
    title: "Stale `#[allow(dead_code)]` on `OAuthConfig::user_agent`"
  - id: IN-02
    severity: info
    disposition: open
    title: "`build_oauth` re-implements every rule of `oauth_problems`"
  - id: IN-03
    severity: info
    disposition: open
    title: "Credential-carrying types derive `Debug`"
  - id: IN-04
    severity: info
    disposition: open
    title: "Reddit settings errors echo the unknown key; the YouTube plugin deliberately does not"
  - id: IN-05
    severity: info
    disposition: open
    title: "Fixture \"is exercised\" check is a substring match"
  - id: IN-06
    severity: info
    disposition: open
    title: "`quota_units_per_poll` is public production code used only by tests"
---

# Phase 07 Review Disposition

All findings default to `open`; triage is recorded by changing a row's disposition. Advisory review: nothing here blocks the phase.
