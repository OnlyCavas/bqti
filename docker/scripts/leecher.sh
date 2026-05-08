#!/bin/bash
set -euo pipefail

NODE_NAME="${NODE_NAME:-leecher}"
log() { echo "[${NODE_NAME} $(date +%T)] $*"; }

rm -f "${BQTI_SOCKET:-/run/bqti/default.sock}"

log "starting daemon on ${BQTI_HOST}:${BQTI_PORT}..."
bqti serve "${BQTI_HOST}:${BQTI_PORT}" --no-cert &
DAEMON_PID=$!

log "waiting for daemon to accept connections..."
set +e

while true; do
    timeout 1 bqti status >/dev/null 2>&1
    _rc=$?
    if [ "$_rc" -eq 0 ] || [ "$_rc" -eq 124 ]; then break; fi
    sleep 0.1
done

set -e
log "daemon ready"

log "waiting for seeder to be ready..."

until [ -f /shared/seeder-ready ]; do sleep 0.5; done
log "seeder ready"

log "adding download..."
bqti download add /shared/sim.torrent
log "download started"

while kill -0 "$DAEMON_PID" 2>/dev/null; do
    timeout 5 bqti status 2>/dev/null || true
    sleep 10
done
