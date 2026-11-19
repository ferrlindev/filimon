#!/usr/bin/env bash
# Registers the compose SPU (id 0) with the compose SC. Idempotent: does nothing
# when the SPU is already registered. Run after `docker compose up -d fluvio-sc fluvio-spu`
# whenever `fluvio cluster spu list` shows "no spu" (the registration is lost when the SC restarts).
# Needs the host `fluvio` CLI. Uses a throwaway profile switch and restores your active profile.
set -euo pipefail
PROFILE="${COMPOSE_FLUVIO_PROFILE:-cpub}"   # host profile for 127.0.0.1:19005
PREV="$(fluvio profile current)"
trap 'fluvio profile switch "$PREV" >/dev/null 2>&1 || true' EXIT
fluvio profile switch "$PROFILE" >/dev/null
if fluvio cluster spu list 2>&1 | grep -q 'Online'; then
  echo "SPU already registered and online"; exit 0
fi
fluvio cluster spu register --id 0 \
  --public-server fluvio-spu:9010 \
  --public-server-local 127.0.0.1:19010 \
  --private-server fluvio-spu:9011
sleep 8
fluvio cluster spu list
