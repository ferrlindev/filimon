# Authoring proof evidence (WIT-03, SC4)

Index for 08-VERIFICATION.md. The transcripts were produced by the user in their own shell (D-12).

- **Tag:** `wit-v0.1.0` (annotated, signed; tag object `cd7e905cd7fabc32e37d01b0ddb4f76db3f6bd05`)
- **Tagged commit:** `31c1deddebc855e5bc5656a89aa16f650d5acbe0` (from `git rev-list -n 1 wit-v0.1.0`)
- **Tag on origin:** confirmed with `git ls-remote --tags origin wit-v0.1.0`
- **Date (UTC):** 2026-11-12
- **Tag message:** `filimon:plugin@0.1.0 authoring contract: wit/, plugins/template, plugins/stub-fetcher, docs/plugin-guide.md`
- **Fetcher fixture:** `plugins/stub-fetcher` stands in for the `plugins/replay-fetcher` named in plan 08-07, per the plan 08-05 Task 1 answer (walkthrough fresh-from-stub). `plugins/replay-fetcher` does exist in the tagged commit (`git cat-file -e wit-v0.1.0:plugins/replay-fetcher/Cargo.toml` succeeds); it was not used for the walkthrough.

## Commands (from the repository root)

Local mode:

```
PROOF_TRANSCRIPT=.planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-local.txt scripts/verify-authoring.sh
```

Remote mode, from the tag:

```
WIT_REF=wit-v0.1.0 PROOF_TRANSCRIPT=.planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-remote.txt scripts/verify-authoring.sh
```

## Transcripts

| File | Mode line | Result line |
|------|-----------|-------------|
| `authoring-proof-local.txt` | `mode: local (git archive HEAD wit)` | `AUTHORING PROOF: PASS (legs: source fetcher)` |
| `authoring-proof-remote.txt` | `mode: remote wit-v0.1.0` | `AUTHORING PROOF: PASS (legs: source fetcher)` |

## Scope

This evidence covers WIT-03 (authoring path) only. The live E2E proof of plan 08-06 is a separate plan and is not covered here.
