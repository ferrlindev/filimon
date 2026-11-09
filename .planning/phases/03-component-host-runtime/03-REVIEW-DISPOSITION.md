---
phase: 03
review: 03-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "No resource limits on guest-controlled memory, item count, cursor size or log volume"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Epoch ticker thread spawn failure is silently swallowed, which removes the anti-wedge guarantee"
  - id: WR-03
    severity: warning
    disposition: open
    title: "Record key is guest-controlled, unbounded and not counted in the size cap; one bad record wedges the plugin in a duplicate-producing retry loop"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Item URL scheme is not restricted"
  - id: WR-05
    severity: warning
    disposition: open
    title: "Guest-chosen `platform` can impersonate another plugin or platform, and the key encoding is ambiguous"
  - id: WR-06
    severity: warning
    disposition: open
    title: "`send_batch` is not cancellation-safe when the sink timeout drops it mid-flight"
  - id: WR-07
    severity: warning
    disposition: open
    title: "An empty `--state-dir` / `FILI_STATE_DIR` silently resolves to the current directory"
  - id: IN-01
    severity: info
    disposition: open
    title: "`clean_guest_text` leaves Unicode line separators and bidi controls in place"
  - id: IN-02
    severity: info
    disposition: open
    title: "Invalid `RUST_LOG` is discarded silently and per-item drop warnings are unbounded"
  - id: IN-03
    severity: info
    disposition: open
    title: "The state-file reader treats a missing `cursor` key as a first poll"
  - id: IN-04
    severity: info
    disposition: open
    title: "The core isolation tests are all `#[ignore]` and depend on a manual fixture build"
---

# Phase 3 code-review disposition

One row per finding in `03-REVIEW.md`, all `open`: the review was advisory and no finding has been triaged yet. Update `disposition` in the frontmatter as each is fixed, deferred or declined.
