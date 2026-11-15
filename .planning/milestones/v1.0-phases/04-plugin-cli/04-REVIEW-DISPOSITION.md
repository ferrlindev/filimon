---
phase: 04-plugin-cli
review: 04-REVIEW.md
status: all-open
---

# Phase 4 code review disposition

Advisory review (standard depth, 19 files): 1 Critical, 3 Warning, 4 Info. Nothing was fixed during execution; every finding is open. Triage with `/gsd-code-review 4 --fix` or in a gap plan.

| # | Severity | Summary | Disposition |
|---|----------|---------|-------------|
| 1 | Critical | run.rs forward_signals: println! before shutdown.cancel() panics on a broken pipe (fili run piped to tee), so no stop request reaches the host | open |
| 2 | Warning | aborted or panicked plugin shows zero polls/items/failures in the run summary | open |
| 3 | Warning | scheduler records ok:true when the sink succeeded but the cursor write failed; run-once --produce records ok:false | open |
| 4 | Warning | guest error text (may echo init secrets) persisted in status file and printed unredacted by list and run-once | open |
| 5 | Info | sink and scheduler comments say send_batch is never dropped, but abort_all now drops it | open |
| 6 | Info | plugins/README.md gives the wrong fili run startup order (Fluvio connect precedes the lock) | open |
| 7 | Info | read_cursor_file has no size bound, unlike read_status | open |
| 8 | Info | check prints miette diagnostics unsanitized | open |
