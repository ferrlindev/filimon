# Phase 02 Deferred Items

## Deferred Items

- docker-compose.yml sets `OUTPUT_PATH` for the `filimon` service, but `fili` never reads it
  status: open
  **What:** `fili consume` takes the output path only from `--output` (default `output/wise_output.json`). Inside the container that resolves to `/app/output/wise_output.json`, which matches the value of the unused env var by accident, since the working directory is `/app`. Found during 02-03 Task 1. README and INTEGRATIONS.md no longer document the env var as working. Fix: drop the env var from compose, or have clap read it with `env = "OUTPUT_PATH"` on `--output`.
- docker-compose.yml `filimon-stream-processor` runs `stream --stage all --from-beginning`, and `fili` has no `stream` subcommand
  status: open
  **What:** This gap predates Phase 2, and the 02-03 plan keeps it out of scope. Only the comment above the service was updated (D-03).
