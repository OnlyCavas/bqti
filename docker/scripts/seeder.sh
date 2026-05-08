#!/bin/bash
set -euo pipefail

log() { echo "[seeder $(date +%T)] $*"; }

rm -f /shared/seeder-ready "${BQTI_SOCKET:-/run/bqti/default.sock}"

if [ -n "${BQTI_BOOTSTRAP:-}" ]; then
    _HOST="${BQTI_BOOTSTRAP%%:*}"
    _PORT="${BQTI_BOOTSTRAP##*:}"
    _IP=$(getent hosts "$_HOST" 2>/dev/null | awk '{print $1}' | head -1)
    if [ -n "$_IP" ]; then
        BOOTSTRAP_ADDR="${_IP}:${_PORT}"
        log "resolved $BQTI_BOOTSTRAP -> $BOOTSTRAP_ADDR"
    else
        BOOTSTRAP_ADDR="$BQTI_BOOTSTRAP"
        log "warning: could not resolve $_HOST, using as-is"
    fi
else
    BOOTSTRAP_ADDR=""
fi

DATA_DIR=/home/appuser/data

if [ ! -d "$DATA_DIR" ] || [ -z "$(ls -A "$DATA_DIR" 2>/dev/null)" ]; then
    log "first run: ingesting source data from /source into $DATA_DIR..."
    mkdir -p "$DATA_DIR"
    cp -r /source/. "$DATA_DIR/"
    log "data ready"
fi

log "creating torrent from $DATA_DIR..."
bqti torrent create "$DATA_DIR" \
    ${BOOTSTRAP_ADDR:+--bootstrap "$BOOTSTRAP_ADDR"} \
    --output /shared/sim.torrent
log "torrent written to /shared/sim.torrent"

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

log "starting seed..."
bqti seed "$DATA_DIR" \
    ${BOOTSTRAP_ADDR:+--bootstrap "$BOOTSTRAP_ADDR"}

touch /shared/seeder-ready
log "seeding — leechers may now start"

wait "$DAEMON_PID"
