#!/usr/bin/env bash
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8188}"
TOKEN="${TOKEN:-e2e-admin-token-change-before-real-use}"
ROOT="${ROOT:-/opt/chronoframe-e2e/app}"
SOURCE="${SOURCE:-/tmp/e2e-large-fixture.png}"
PROJECT_NAME="${PROJECT_NAME:-app}"
DATA_VOLUME="${PROJECT_NAME}_chronoframe-data"
HDR=(-H "X-Admin-Token: $TOKEN")
COMPOSE=(docker compose --project-name "$PROJECT_NAME" -f "$ROOT/docker-compose.e2e.yml")
if [[ -n "${COMPOSE_OVERRIDE:-}" ]]; then COMPOSE+=(-f "$COMPOSE_OVERRIDE"); fi

api() { curl -fsS --connect-timeout 5 --max-time 300 "${HDR[@]}" "$@"; }
json_value() { python3 -c "import json,sys; value=json.load(sys.stdin)$1; print(value)"; }

[[ -s "$SOURCE" ]] || python3 "$ROOT/scripts/make-fixture.py" "$SOURCE"
album=$(api -H 'Content-Type: application/json' -d '{"name":"E2E deletion crash recovery"}' "$BASE/api/albums" | json_value "['id']")
parts=()
for i in $(seq 1 48); do parts+=(-F "files=@$SOURCE;filename=delete-crash-$i.png"); done
upload_response=$(api -X POST "${parts[@]}" "$BASE/api/albums/$album/photos")
printf '%s' "$upload_response" | python3 -c 'import json,sys,pathlib; photos=json.load(sys.stdin); assert len(photos)==48; pathlib.Path("/tmp/e2e-delete-source-ids").write_text("\n".join(p["id"] for p in photos))'
job=$(api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$album\"],\"targetFormat\":\"webp\"}" "$BASE/api/conversions" | json_value "['id']")
for _ in $(seq 1 2400); do
  snapshot=$(api "$BASE/api/conversions/$job?items=false")
  status=$(printf '%s' "$snapshot" | json_value "['job']['status']")
  [[ "$status" = completed ]] && break
  [[ "$status" = failed ]] && { echo "$snapshot" >&2; exit 1; }
  sleep 0.1
done
[[ "${status:-}" = completed ]] || { echo "conversion did not complete" >&2; exit 1; }

curl -fsS --connect-timeout 5 --max-time 300 "${HDR[@]}" -X DELETE "$BASE/api/conversions/$job/delete-sources" >/tmp/e2e-delete-response.json 2>/tmp/e2e-delete-response.err &
delete_pid=$!
prepared=0
for _ in $(seq 1 4000); do
  marker=$(api "$BASE/api/conversions/$job?items=false" | json_value "['job']['sourcesDeletedAt']")
  if [[ "$marker" = -* ]]; then prepared=1; break; fi
  sleep 0.01
done
[[ "$prepared" = 1 ]] || { wait "$delete_pid" || true; echo "durable deletion state was not observed" >&2; exit 1; }
"${COMPOSE[@]}" kill -s SIGKILL chronoframe >/dev/null
wait "$delete_pid" || true
"${COMPOSE[@]}" up -d chronoframe >/dev/null

ready=0
for _ in $(seq 1 180); do
  if curl -fsS --connect-timeout 2 --max-time 4 "$BASE/api/albums" >/dev/null 2>&1; then ready=1; break; fi
  sleep 1
done
[[ "$ready" = 1 ]] || { "${COMPOSE[@]}" logs --tail=200 chronoframe >&2; exit 1; }

api "$BASE/api/conversions/$job?items=false" | python3 -c 'import json,sys; job=json.load(sys.stdin)["job"]; assert job["sourcesDeletedAt"] and job["sourcesDeletedAt"] > 0, job'
api "$BASE/api/albums/$album/photos" >/tmp/e2e-delete-photos.json
python3 - <<'PY'
import json, pathlib
photos = json.loads(pathlib.Path('/tmp/e2e-delete-photos.json').read_text())
assert len(photos) == 48, len(photos)
assert all(photo['format'] == 'webp' for photo in photos)
PY
while IFS= read -r source_id; do
  code=$(curl -sS --connect-timeout 5 --max-time 30 -o /dev/null -w '%{http_code}' "$BASE/api/photos/$source_id/file")
  [[ "$code" = 404 ]] || { echo "source $source_id returned HTTP $code" >&2; exit 1; }
done </tmp/e2e-delete-source-ids

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
"${COMPOSE[@]}" logs --tail=300 chronoframe 2>&1 | grep -q 'replayed source deletion outbox'
echo "PASS source-deletion outbox replay after SIGKILL"
