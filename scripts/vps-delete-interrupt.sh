#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE="${BASE:-http://127.0.0.1:8188}"
ADMIN_USER="${ADMIN_USER:-e2e-admin}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-E2E-only-password-2026!}"
ROOT="${ROOT:-/opt/chronoframe-e2e/app}"
SOURCE="${SOURCE:-/tmp/e2e-large-fixture.png}"
PROJECT_NAME="${PROJECT_NAME:-app}"
DATA_VOLUME="${PROJECT_NAME}_chronoframe-data"
RUN_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/chronoframe-delete-e2e.XXXXXX")
chmod 700 "$RUN_TMP_DIR"
COOKIE_JAR="$RUN_TMP_DIR/session.cookies"
CSRF_TOKEN=""
COMPOSE=(docker compose --project-name "$PROJECT_NAME" -f "$ROOT/docker-compose.e2e.yml")
if [[ -n "${COMPOSE_OVERRIDE:-}" ]]; then COMPOSE+=(-f "$COMPOSE_OVERRIDE"); fi

cleanup() {
  if [[ -n "${RUN_TMP_DIR:-}" && -d "$RUN_TMP_DIR" ]]; then rm -rf -- "$RUN_TMP_DIR"; fi
}
trap cleanup EXIT

auth_curl() {
  curl -b "$COOKIE_JAR" \
    -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF_TOKEN" "$@"
}
auth_cookie_curl() {
  curl -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF_TOKEN" "$@"
}
api() { auth_curl -fsS --connect-timeout 5 --max-time 300 "$@"; }
json_value() { python3 -c "import json,sys; value=json.load(sys.stdin)$1; print(value)"; }

wait_app_ready() {
  local ready=0
  for _ in $(seq 1 180); do
    if curl -fsS --connect-timeout 2 --max-time 4 "$BASE/api/albums" >/dev/null 2>&1; then ready=1; break; fi
    sleep 1
  done
  [[ "$ready" = 1 ]] || { "${COMPOSE[@]}" logs --tail=200 chronoframe >&2 || true; echo "ChronoFrame did not become ready" >&2; exit 1; }
}

: >"$COOKIE_JAR"
chmod 600 "$COOKIE_JAR"
"${COMPOSE[@]}" up -d >/dev/null
wait_app_ready
initialized=$(curl -fsS "$BASE/api/auth/status" | json_value "['initialized']")
auth_endpoint=login
[[ "$initialized" = False ]] && auth_endpoint=register
auth_cookie_curl -fsS --connect-timeout 5 --max-time 60 \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASSWORD\"}" \
  "$BASE/api/auth/$auth_endpoint" >"$RUN_TMP_DIR/auth.json"
CSRF_TOKEN=$(awk '$6 == "cf_csrf" { value=$7 } END { print value }' "$COOKIE_JAR")
[[ -n "$CSRF_TOKEN" ]] || { echo "authentication response did not set cf_csrf" >&2; exit 1; }

[[ -s "$SOURCE" ]] || python3 "$ROOT/scripts/make-fixture.py" "$SOURCE"
album=$(api -H 'Content-Type: application/json' -d '{"name":"E2E deletion crash recovery"}' "$BASE/api/albums" | json_value "['id']")
parts=()
for i in $(seq 1 48); do parts+=(-F "files=@$SOURCE;filename=delete-crash-$i.png"); done
upload_response=$(api -X POST "${parts[@]}" "$BASE/api/albums/$album/photos")
printf '%s' "$upload_response" | python3 -c 'import json,sys,pathlib; photos=json.load(sys.stdin); assert len(photos)==48; pathlib.Path(sys.argv[1]).write_text("\n".join(p["id"] for p in photos))' "$RUN_TMP_DIR/source-ids"
job=$(api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$album\"],\"targetFormat\":\"webp\"}" "$BASE/api/conversions" | json_value "['id']")
for _ in $(seq 1 2400); do
  snapshot=$(api "$BASE/api/conversions/$job?items=false")
  status=$(printf '%s' "$snapshot" | json_value "['job']['status']")
  [[ "$status" = completed ]] && break
  [[ "$status" = failed ]] && { echo "$snapshot" >&2; exit 1; }
  sleep 0.1
done
[[ "${status:-}" = completed ]] || { echo "conversion did not complete" >&2; exit 1; }

auth_curl -fsS --connect-timeout 5 --max-time 300 -X DELETE "$BASE/api/conversions/$job/delete-sources" >"$RUN_TMP_DIR/delete-response.json" 2>"$RUN_TMP_DIR/delete-response.err" &
delete_pid=$!
prepared=0
completed_before_interrupt=0
for _ in $(seq 1 4000); do
  marker=$(api "$BASE/api/conversions/$job?items=false" | json_value "['job']['sourcesDeletedAt']")
  if [[ "$marker" = -* ]]; then prepared=1; break; fi
  if [[ "$marker" =~ ^[0-9]+$ ]] && (( marker > 0 )); then completed_before_interrupt=1; break; fi
  sleep 0.01
done
if [[ "$prepared" = 1 ]]; then
  "${COMPOSE[@]}" kill -s SIGKILL chronoframe >/dev/null
  wait "$delete_pid" || true
  "${COMPOSE[@]}" up -d chronoframe >/dev/null
elif [[ "$completed_before_interrupt" = 1 ]]; then
  wait "$delete_pid"
  "${COMPOSE[@]}" restart chronoframe >/dev/null
else
  wait "$delete_pid" || true
  echo "durable deletion state was not observed" >&2
  exit 1
fi
wait_app_ready

api "$BASE/api/conversions/$job?items=false" | python3 -c 'import json,sys; job=json.load(sys.stdin)["job"]; assert job["sourcesDeletedAt"] and job["sourcesDeletedAt"] > 0, job'
api "$BASE/api/albums/$album/photos" >"$RUN_TMP_DIR/photos.json"
python3 - "$RUN_TMP_DIR/photos.json" <<'PY'
import json, pathlib, sys
photos = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert len(photos) == 48, len(photos)
assert all(photo['format'] == 'webp' for photo in photos)
PY
while IFS= read -r source_id; do
  code=$(curl -sS --connect-timeout 5 --max-time 30 -o /dev/null -w '%{http_code}' "$BASE/api/photos/$source_id/file")
  [[ "$code" = 404 ]] || { echo "source $source_id returned HTTP $code" >&2; exit 1; }
done <"$RUN_TMP_DIR/source-ids"

database_path="$(docker volume inspect -f '{{.Mountpoint}}' "$DATA_VOLUME")/chronoframe.db"
python3 - "$database_path" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    assert db.execute('select count(*) from source_deletion_outbox').fetchone()[0] == 0
    assert db.execute('select count(*) from pending_blobs').fetchone()[0] == 0
PY
chronoframe_container=$("${COMPOSE[@]}" ps -q chronoframe)
found=$(docker exec "$chronoframe_container" find /app/data -type f -name '*.cf-pending' -print)
[[ -z "$found" ]] || { echo "temporary files remain: $found" >&2; exit 1; }
"${COMPOSE[@]}" logs --tail=300 chronoframe >"$RUN_TMP_DIR/chronoframe.log" 2>&1
if grep -q 'replayed source deletion outbox' "$RUN_TMP_DIR/chronoframe.log"; then
  echo "PASS source-deletion outbox replay after SIGKILL"
else
  echo "PASS source deletion completed safely before the interrupt window closed"
fi
