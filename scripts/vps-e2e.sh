#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE="${BASE:-http://127.0.0.1:8188}"
ADMIN_USER="${ADMIN_USER:-e2e-admin}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-E2E-only-password-2026!}"
AUTH_USER="$ADMIN_USER"
ROOT="${ROOT:-/opt/chronoframe-e2e/app}"
SOURCE="${SOURCE:-$ROOT/public/favicon-96x96.png}"
LARGE_SOURCE="${LARGE_SOURCE:-/tmp/e2e-large-fixture.png}"
PROJECT_NAME="${PROJECT_NAME:-app}"
NETWORK_NAME="${PROJECT_NAME}_default"
DATA_VOLUME="${PROJECT_NAME}_chronoframe-data"
RUN_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/chronoframe-e2e.XXXXXX")
chmod 700 "$RUN_TMP_DIR"
COOKIE_JAR="$RUN_TMP_DIR/session.cookies"
CSRF_TOKEN=""
COMPOSE=(docker compose --project-name "$PROJECT_NAME" -f "$ROOT/docker-compose.e2e.yml")
if [[ -n "${COMPOSE_OVERRIDE:-}" ]]; then COMPOSE+=(-f "$COMPOSE_OVERRIDE"); fi
LOAD_PID=""

fail() { echo "FAIL: $*" >&2; exit 1; }
cleanup() {
  if [[ -n "$LOAD_PID" ]] && kill -0 "$LOAD_PID" 2>/dev/null; then kill "$LOAD_PID" 2>/dev/null || true; fi
  if [[ -n "${RUN_TMP_DIR:-}" && -d "$RUN_TMP_DIR" ]]; then rm -rf -- "$RUN_TMP_DIR"; fi
}
trap cleanup EXIT

json_value() { python3 -c "import json,sys; value=json.load(sys.stdin)$1; print(value)"; }
auth_curl() {
  curl -b "$COOKIE_JAR" \
    -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF_TOKEN" "$@"
}
auth_cookie_curl() {
  curl -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF_TOKEN" "$@"
}
api() { auth_curl -fsS --connect-timeout 5 --max-time 180 "$@"; }
make_album() { api -H 'Content-Type: application/json' -d "{\"name\":\"$1\"}" "$BASE/api/albums" | json_value "['id']"; }
upload() { api -X POST -F "files=@$SOURCE;filename=$2" "$BASE/api/albums/$1/photos"; }
upload_file() { api -X POST -F "files=@$2;filename=$3" "$BASE/api/albums/$1/photos"; }
start_job() { api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$1\"],\"targetFormat\":\"$2\"}" "$BASE/api/conversions" | json_value "['id']"; }
start_multi_job() { api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$1\",\"$2\"],\"targetFormat\":\"$3\"}" "$BASE/api/conversions" | json_value "['id']"; }
job_status() { api "$BASE/api/conversions/$1?items=false" | json_value "['job']['status']"; }
settings() { api -X PUT -H 'Content-Type: application/json' -d "$1" "$BASE/api/settings/storage"; }

clear_auth() {
  rm -f "$COOKIE_JAR"
  : >"$COOKIE_JAR"
  chmod 600 "$COOKIE_JAR"
  CSRF_TOKEN=""
}

refresh_csrf() {
  chmod 600 "$COOKIE_JAR"
  CSRF_TOKEN=$(awk '$6 == "cf_csrf" { value=$7 } END { print value }' "$COOKIE_JAR")
  [[ -n "$CSRF_TOKEN" ]] || fail "authentication response did not set cf_csrf"
}

session_cookie_header() {
  awk '$6 == "cf_session" || $6 == "cf_csrf" { values[$6]=$7 } END { separator=""; for (name in values) { printf "%s%s=%s", separator, name, values[name]; separator="; " } }' "$COOKIE_JAR"
}

session_token_from_jar() {
  awk '$6 == "cf_session" { value=$7 } END { print value }' "$COOKIE_JAR"
}

register_admin() {
  auth_cookie_curl -fsS --connect-timeout 5 --max-time 60 \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$AUTH_USER\",\"password\":\"$ADMIN_PASSWORD\"}" \
    "$BASE/api/auth/register" >"$RUN_TMP_DIR/register.json"
  refresh_csrf
}

login_admin() {
  clear_auth
  auth_cookie_curl -fsS --connect-timeout 5 --max-time 60 \
    -D "$RUN_TMP_DIR/login.headers" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$AUTH_USER\",\"password\":\"$ADMIN_PASSWORD\"}" \
    "$BASE/api/auth/login" >"$RUN_TMP_DIR/login.json"
  refresh_csrf
}

wait_app_ready() {
  local ready=0
  for _ in $(seq 1 90); do
    if curl -fsS --connect-timeout 2 --max-time 4 "$BASE/api/albums" >/dev/null 2>&1; then ready=1; break; fi
    sleep 1
  done
  [[ "$ready" = 1 ]] || { "${COMPOSE[@]}" logs --tail=120 chronoframe >&2 || true; fail "ChronoFrame did not become ready"; }
}

wait_webdav_ready() {
  local ready=0 container ip
  for _ in $(seq 1 60); do
    container=$("${COMPOSE[@]}" ps -q webdav 2>/dev/null || true)
    if [[ -n "$container" ]]; then
      ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$container" 2>/dev/null || true)
      if [[ -n "$ip" ]] && curl -fsS --connect-timeout 2 --max-time 4 -u 'e2e-webdav-user:e2e-webdav-password' "http://$ip/" >/dev/null 2>&1; then ready=1; break; fi
    fi
    sleep 1
  done
  [[ "$ready" = 1 ]] || fail "WebDAV did not become ready"
}

wait_minio_ready() {
  local ready=0 container ip
  for _ in $(seq 1 60); do
    container=$("${COMPOSE[@]}" ps -q minio 2>/dev/null || true)
    if [[ -n "$container" ]]; then
      ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$container" 2>/dev/null || true)
      if [[ -n "$ip" ]] && curl -fsS --connect-timeout 2 --max-time 4 "http://$ip:9000/minio/health/ready" >/dev/null 2>&1; then ready=1; break; fi
    fi
    sleep 1
  done
  [[ "$ready" = 1 ]] || fail "MinIO did not become ready"
}

ensure_test_bucket() {
  wait_minio_ready
  docker run --rm --network "$NETWORK_NAME" --entrypoint /bin/sh minio/mc:latest -c \
    'mc alias set e2e http://minio:9000 e2e-minio-access e2e-minio-secret-change-me >/dev/null && mc mb --ignore-existing e2e/chronoframe-e2e >/dev/null'
}

reset_stack() {
  local initialize_auth=${1:-yes}
  [[ -d "$ROOT" ]] || fail "isolated E2E directory is missing"
  cd "$ROOT"
  [[ "$(pwd -P)" = "$ROOT" ]] || fail "refusing to reset outside $ROOT"
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null
  "${COMPOSE[@]}" up -d >/dev/null
  wait_app_ready
  wait_webdav_ready
  ensure_test_bucket
  clear_auth
  AUTH_USER="$ADMIN_USER"
  if [[ "$initialize_auth" = yes ]]; then register_admin; fi
}

wait_job() {
  local job=$1 timeout_seconds=${2:-300} status
  for _ in $(seq 1 $((timeout_seconds * 4))); do
    status=$(job_status "$job")
    case "$status" in
      completed|cancelled|interrupted|failed) echo "$status"; return 0 ;;
    esac
    sleep 0.25
  done
  fail "job $job did not finish in ${timeout_seconds}s"
}

wait_for_running() {
  local job=$1 timeout_seconds=${2:-30} snapshot status
  for _ in $(seq 1 $((timeout_seconds * 10))); do
    snapshot=$(api "$BASE/api/conversions/$job?items=false")
    printf '%s' "$snapshot" | python3 -c 'import json,sys; j=json.load(sys.stdin)["job"]; assert j["completed"] == j["succeeded"] + j["failed"] + j["cancelled"]; assert 0 <= j["completed"] <= j["total"]'
    status=$(printf '%s' "$snapshot" | json_value "['job']['status']")
    [[ "$status" = running ]] && return 0
    case "$status" in completed|cancelled|interrupted|failed) fail "job $job reached $status before running was observed";; esac
    sleep 0.1
  done
  fail "job $job never entered running state"
}

wait_for_partial_progress() {
  local job=$1 timeout_seconds=${2:-180} snapshot status completed total previous=0
  for _ in $(seq 1 $((timeout_seconds * 10))); do
    snapshot=$(api "$BASE/api/conversions/$job?items=false")
    printf '%s' "$snapshot" | python3 -c 'import json,sys; j=json.load(sys.stdin)["job"]; assert j["completed"] == j["succeeded"] + j["failed"] + j["cancelled"]; assert 0 <= j["completed"] <= j["total"]'
    status=$(printf '%s' "$snapshot" | json_value "['job']['status']")
    completed=$(printf '%s' "$snapshot" | json_value "['job']['completed']")
    total=$(printf '%s' "$snapshot" | json_value "['job']['total']")
    (( completed >= previous )) || fail "job $job progress went backwards ($previous -> $completed)"
    previous=$completed
    if [[ "$status" = running ]] && (( completed > 0 && completed < total )); then echo "$completed"; return 0; fi
    case "$status" in completed|cancelled|interrupted|failed) fail "job $job reached $status before partial progress was observed";; esac
    sleep 0.1
  done
  fail "job $job had no observable intermediate progress"
}

assert_signature() {
  python3 - "$1" "$2" <<'PY'
import pathlib, sys
path, expected = pathlib.Path(sys.argv[1]), sys.argv[2]
data = path.read_bytes()
checks = {
    "png": data.startswith(b"\x89PNG\r\n\x1a\n") and data.endswith(b"IEND\xaeB`\x82"),
    "jpg": data.startswith(b"\xff\xd8\xff") and data.endswith(b"\xff\xd9"),
    "webp": len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP",
}
assert checks.get(expected, False), f"{path} is not a valid-looking {expected} file"
PY
}

assert_completed_job() {
  local job=$1 expected_total=$2 expected_format=$3 expected_names=${4:--} snapshot_file="$RUN_TMP_DIR/job-$1.json"
  api "$BASE/api/conversions/$job" >"$snapshot_file"
  python3 - "$snapshot_file" "$expected_total" "$expected_format" "$expected_names" <<'PY'
import json, pathlib, sys
payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
expected_total, expected_format, expected_names = int(sys.argv[2]), sys.argv[3], sys.argv[4]
job, items = payload["job"], payload["items"]
assert job["status"] == "completed", job
assert job["targetFormat"] == expected_format, job
assert job["total"] == expected_total, job
assert job["completed"] == job["succeeded"] == expected_total, job
assert job["failed"] == job["cancelled"] == 0, job
assert job.get("sourcesDeletedAt") is None, job
assert len(items) == expected_total, (len(items), expected_total)
assert all(item["status"] == "succeeded" for item in items), items
assert all(item.get("targetPhotoId") and not item.get("error") for item in items), items
assert len({item["targetPhotoId"] for item in items}) == expected_total, items
if expected_names != "-":
    assert sorted(item["sourceName"] for item in items) == sorted(expected_names.split("|")), items
PY
  mapfile -t target_ids < <(python3 -c 'import json,sys; print("\n".join(i["targetPhotoId"] for i in json.load(open(sys.argv[1]))["items"]))' "$snapshot_file")
  [[ "${#target_ids[@]}" = "$expected_total" ]] || fail "job $job target count mismatch"
  local index=0 output
  for target_id in "${target_ids[@]}"; do
    output="$RUN_TMP_DIR/output-$job-$index.$expected_format"
    curl -fsS --connect-timeout 5 --max-time 180 "$BASE/api/photos/$target_id/file" -o "$output"
    assert_signature "$output" "$expected_format"
    index=$((index + 1))
  done
}

first_job_target() {
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["items"][0]["targetPhotoId"])' "$RUN_TMP_DIR/job-$1.json"
}

assert_cancelled_job() {
  local job=$1 expected_total=$2 minimum_completed=$3 snapshot_file="$RUN_TMP_DIR/job-$1.json"
  api "$BASE/api/conversions/$job" >"$snapshot_file"
  python3 - "$snapshot_file" "$expected_total" "$minimum_completed" <<'PY'
import json, pathlib, sys
payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
total, minimum = int(sys.argv[2]), int(sys.argv[3])
job, items = payload["job"], payload["items"]
assert job["status"] == "cancelled", job
assert job["total"] == job["completed"] == total, job
assert job["completed"] == job["succeeded"] + job["failed"] + job["cancelled"], job
assert minimum <= job["succeeded"] < total, job
assert job["failed"] == 0 and job["cancelled"] == total - job["succeeded"], job
assert len(items) == total, len(items)
assert sum(i["status"] == "succeeded" for i in items) == job["succeeded"], items
assert sum(i["status"] == "cancelled" for i in items) == job["cancelled"], items
assert all(i["status"] in {"succeeded", "cancelled"} for i in items), items
assert all(i.get("targetPhotoId") for i in items if i["status"] == "succeeded"), items
assert all(not i.get("targetPhotoId") for i in items if i["status"] == "cancelled"), items
assert all(i.get("sourcePhotoId") for i in items), items
PY
}

assert_interrupted_job() {
  local job=$1 expected_total=$2 minimum_completed=$3 snapshot_file="$RUN_TMP_DIR/job-$1.json"
  api "$BASE/api/conversions/$job" >"$snapshot_file"
  python3 - "$snapshot_file" "$expected_total" "$minimum_completed" <<'PY'
import json, pathlib, sys
payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
total, minimum = int(sys.argv[2]), int(sys.argv[3])
job, items = payload["job"], payload["items"]
assert job["status"] == "interrupted", job
assert job["total"] == total and minimum <= job["completed"] <= total, job
assert job["completed"] == job["succeeded"] + job["failed"] + job["cancelled"], job
assert job["failed"] == 0, job
assert len(items) == total, len(items)
assert sum(i["status"] == "succeeded" for i in items) == job["succeeded"], items
assert sum(i["status"] in {"succeeded", "failed", "cancelled"} for i in items) == job["completed"], items
assert all(i.get("targetPhotoId") for i in items if i["status"] == "succeeded"), items
assert all(i.get("sourcePhotoId") for i in items), items
PY
  mapfile -t target_ids < <(python3 -c 'import json,sys; print("\n".join(i["targetPhotoId"] for i in json.load(open(sys.argv[1]))["items"] if i["status"]=="succeeded"))' "$snapshot_file")
  local index=0 output
  for target_id in "${target_ids[@]}"; do
    output="$RUN_TMP_DIR/interrupted-$job-$index.webp"
    curl -fsS --connect-timeout 5 --max-time 180 "$BASE/api/photos/$target_id/file" -o "$output"
    assert_signature "$output" webp
    index=$((index + 1))
  done
}

assert_sources_retained() {
  local job=$1 album=$2 expected=$3 job_file="$RUN_TMP_DIR/job-$1.json" photos_file="$RUN_TMP_DIR/photos-$1.json"
  api "$BASE/api/albums/$album/photos" >"$photos_file"
  python3 - "$job_file" "$photos_file" "$expected" <<'PY'
import json, pathlib, sys
job = json.loads(pathlib.Path(sys.argv[1]).read_text())
photos = json.loads(pathlib.Path(sys.argv[2]).read_text())
expected = int(sys.argv[3])
sources = {i["sourcePhotoId"] for i in job["items"]}
available = {p["id"] for p in photos}
assert len(sources) == expected and sources <= available, (len(sources), len(available))
assert sum(p["originalName"].startswith("stress-") and p["format"] == "png" for p in photos) == expected
PY
  mapfile -t sample_sources < <(python3 -c 'import json,sys; print("\n".join(i["sourcePhotoId"] for i in json.load(open(sys.argv[1]))["items"][:4]))' "$job_file")
  local index=0 output
  for source_id in "${sample_sources[@]}"; do
    output="$RUN_TMP_DIR/retained-$job-$index.png"
    curl -fsS --connect-timeout 5 --max-time 180 "$BASE/api/photos/$source_id/file" -o "$output"
    assert_signature "$output" png
    index=$((index + 1))
  done
}

assert_delete_sources() {
  local job=$1 expected=$2 body
  body=$(api -X DELETE "$BASE/api/conversions/$job/delete-sources")
  printf '%s' "$body" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["removed"] == int(sys.argv[1]), value; assert value["failures"] == [], value' "$expected"
  api "$BASE/api/conversions/$job?items=false" | python3 -c 'import json,sys; assert json.load(sys.stdin)["job"]["sourcesDeletedAt"] is not None'
}

assert_secret_hidden() {
  local secret=$1 flag=$2 payload
  payload=$(api "$BASE/api/settings/storage")
  printf '%s' "$payload" | python3 -c 'import json,sys; payload=json.load(sys.stdin); raw=json.dumps(payload); assert sys.argv[1] not in raw; assert payload[sys.argv[2]] is True, payload' "$secret" "$flag"
}

assert_secret_encrypted() {
  local key=$1 secret=$2 current_database
  current_database="$(docker volume inspect -f '{{.Mountpoint}}' "$DATA_VOLUME")/chronoframe.db"
  python3 - "$current_database" "$key" "$secret" <<'PY'
import base64, sqlite3, sys
with sqlite3.connect(sys.argv[1], timeout=10) as db:
    value = db.execute('select value from app_settings where key=?', (sys.argv[2],)).fetchone()[0]
assert value != sys.argv[3] and sys.argv[3] not in value
payload = base64.b64decode(value, validate=True)
assert len(payload) >= 12 + 16, len(payload)
PY
}

assert_no_local_temps() {
  local container found
  container=$("${COMPOSE[@]}" ps -q chronoframe)
  found=$(docker exec "$container" find /app/data -type f \( -name '.upload-*' -o -name '*.tmp-*' -o -name '*.cf-pending' \) -print)
  [[ -z "$found" ]] || fail "local temporary objects remain: $found"
}

assert_no_webdav_temps() {
  local container found
  container=$("${COMPOSE[@]}" ps -q webdav)
  found=$(docker exec "$container" find /var/lib/dav -type f \( -name '.upload-*' -o -name '*.tmp-*' -o -name '*.cf-pending' \) -print)
  [[ -z "$found" ]] || fail "WebDAV temporary objects remain: $found"
}

assert_no_s3_temps() {
  local found
  found=$(docker run --rm --network "$NETWORK_NAME" --entrypoint /bin/sh minio/mc:latest -c \
    'mc alias set e2e http://minio:9000 e2e-minio-access e2e-minio-secret-change-me >/dev/null && { mc find e2e/chronoframe-e2e/e2e --name "*.tmp-*"; mc find e2e/chronoframe-e2e/e2e --name "*.cf-pending"; }')
  [[ -z "$found" ]] || fail "S3 temporary objects remain: $found"
}

assert_no_export_temps() {
  local container found=""
  container=$("${COMPOSE[@]}" ps -q chronoframe)
  for _ in $(seq 1 40); do
    found=$(docker exec "$container" find /tmp -maxdepth 1 -type d -name 'chronoframe-export-*' -print)
    [[ -z "$found" ]] && return 0
    sleep 0.1
  done
  fail "album export temporary directories remain: $found"
}

LOCAL_SETTINGS='{"backend":"local","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
WEBDAV_SETTINGS='{"backend":"webdav","localPath":"/app/data/e2e-local","webdavUrl":"http://webdav/","webdavUsername":"e2e-webdav-user","webdavPassword":"e2e-webdav-password","webdavPrefix":"e2e","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
WEBDAV_BAD_SETTINGS='{"backend":"webdav","localPath":"/app/data/e2e-local","webdavUrl":"http://webdav/","webdavUsername":"e2e-webdav-user","webdavPassword":"definitely-wrong","webdavPrefix":"e2e","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
S3_SETTINGS='{"backend":"s3","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"http://minio:9000","s3Region":"us-east-1","s3Bucket":"chronoframe-e2e","s3AccessKey":"e2e-minio-access","s3SecretKey":"e2e-minio-secret-change-me","s3Prefix":"e2e"}'
S3_BAD_SETTINGS='{"backend":"s3","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"http://minio:9000","s3Region":"us-east-1","s3Bucket":"chronoframe-e2e","s3AccessKey":"e2e-minio-access","s3SecretKey":"definitely-wrong","s3Prefix":"e2e"}'

# Start from clean, project-scoped Docker volumes and prove the static Nuxt app plus the complete first-admin boundary.
reset_stack noauth
for route in / /photos /albums /dashboard; do
  page=$(curl -fsS --connect-timeout 5 --max-time 30 "$BASE$route")
  grep -q 'id="__nuxt"' <<<"$page"
done
unknown_api=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/api-not-found.json" -w '%{http_code}' "$BASE/api/does-not-exist")
[[ "$unknown_api" = 404 ]] || fail "unknown API route returned HTTP $unknown_api"
python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))["error"]' "$RUN_TMP_DIR/api-not-found.json"
missing_asset=$(curl -sS --connect-timeout 5 --max-time 30 -o /dev/null -w '%{http_code}' "$BASE/_nuxt/does-not-exist.js")
[[ "$missing_asset" = 404 ]] || fail "missing Nuxt asset returned HTTP $missing_asset"
unauthorized=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/unauthorized.json" -w '%{http_code}' -H 'Content-Type: application/json' -d '{"name":"forbidden"}' "$BASE/api/albums")
[[ "$unauthorized" = 401 ]] || fail "unauthenticated mutation returned HTTP $unauthorized"
curl -fsS "$BASE/api/auth/status" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value == {"initialized": False, "authenticated": False}, value'

weak_registration=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/weak-registration.json" -w '%{http_code}' \
  -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"short"}' "$BASE/api/auth/register")
[[ "$weak_registration" = 400 ]] || fail "weak first-admin password returned HTTP $weak_registration"
long_username=$(printf 'x%.0s' $(seq 1 65))
invalid_username=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/invalid-username.json" -w '%{http_code}' \
  -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d "{\"username\":\"$long_username\",\"password\":\"$ADMIN_PASSWORD\"}" "$BASE/api/auth/register")
[[ "$invalid_username" = 400 ]] || fail "overlong first-admin username returned HTTP $invalid_username"
missing_requested_with=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/missing-requested-with.json" -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASSWORD\"}" "$BASE/api/auth/register")
[[ "$missing_requested_with" = 403 ]] || fail "registration without X-Requested-With returned HTTP $missing_requested_with"
uninitialized_login=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/uninitialized-login.json" -w '%{http_code}' \
  -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASSWORD\"}" "$BASE/api/auth/login")
[[ "$uninitialized_login" = 409 ]] || fail "login before first registration returned HTTP $uninitialized_login"
curl -fsS "$BASE/api/auth/status" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert not value["initialized"] and not value["authenticated"], value'

register_pids=()
register_race_dir="$RUN_TMP_DIR/registration-race"
mkdir -m 700 "$register_race_dir"
for i in $(seq 1 12); do
  (
    curl -sS --connect-timeout 5 --max-time 90 -o "$register_race_dir/register-$i.json" -w '%{http_code}' \
      -c "$register_race_dir/register-$i.cookies" -H 'X-Requested-With: ChronoFrame' \
      -H 'Content-Type: application/json' \
      -d "{\"username\":\"e2e-race-$i\",\"password\":\"$ADMIN_PASSWORD\"}" \
      "$BASE/api/auth/register" >"$register_race_dir/register-$i.code"
  ) &
  register_pids+=("$!")
done
for pid in "${register_pids[@]}"; do wait "$pid"; done
AUTH_USER=$(python3 - "$register_race_dir" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
entries = [(path, path.read_text().strip()) for path in root.glob('register-*.code')]
assert len(entries) == 12, entries
codes = [code for _, code in entries]
assert codes.count('201') == 1, codes
assert codes.count('409') == 11, codes
assert set(codes) == {'201', '409'}, codes
winner = next(path for path, code in entries if code == '201')
print(f"e2e-race-{winner.stem.split('-', 1)[1]}")
PY
)

wrong_login=$(curl -sS --connect-timeout 5 --max-time 60 -o "$RUN_TMP_DIR/wrong-login.json" -w '%{http_code}' \
  -c "$RUN_TMP_DIR/wrong-login.cookies" -H 'X-Requested-With: ChronoFrame' \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$AUTH_USER\",\"password\":\"definitely-wrong-password\"}" \
  "$BASE/api/auth/login")
[[ "$wrong_login" = 401 ]] || fail "wrong password returned HTTP $wrong_login"
login_admin
auth_curl -fsS "$BASE/api/auth/status" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["initialized"] and value["authenticated"] and value["username"], value'
python3 - "$RUN_TMP_DIR/login.headers" <<'PY'
from pathlib import Path
import sys
headers = Path(sys.argv[1]).read_text().lower()
session = next(line for line in headers.splitlines() if line.startswith('set-cookie: cf_session='))
csrf = next(line for line in headers.splitlines() if line.startswith('set-cookie: cf_csrf='))
assert 'httponly' in session and 'samesite=strict' in session and 'path=/' in session and 'max-age=' in session, session
assert 'httponly' not in csrf and 'samesite=strict' in csrf and 'path=/' in csrf and 'max-age=' in csrf, csrf
assert 'domain=' not in session and 'domain=' not in csrf
assert '; secure' not in session and '; secure' not in csrf
PY

# Construction-level proxy-header check only. A real TLS reverse-proxy/browser check remains a deployment test.
curl -fsS --connect-timeout 5 --max-time 60 -D "$RUN_TMP_DIR/https-proxy.headers" \
  -c "$RUN_TMP_DIR/https-proxy.cookies" -H 'X-Requested-With: ChronoFrame' \
  -H 'X-Forwarded-Proto: https' -H 'Content-Type: application/json' \
  -d "{\"username\":\"$AUTH_USER\",\"password\":\"$ADMIN_PASSWORD\"}" \
  "$BASE/api/auth/login" >"$RUN_TMP_DIR/https-proxy-login.json"
python3 - "$RUN_TMP_DIR/https-proxy.headers" <<'PY'
from pathlib import Path
import sys
headers = Path(sys.argv[1]).read_text().lower()
cookies = [line for line in headers.splitlines() if line.startswith('set-cookie: cf_')]
assert len(cookies) == 2 and all('; secure' in cookie for cookie in cookies), cookies
PY

database_path="$(docker volume inspect -f '{{.Mountpoint}}' "$DATA_VOLUME")/chronoframe.db"
master_key_path="$(docker volume inspect -f '{{.Mountpoint}}' "$DATA_VOLUME")/secret.key"
python3 - "$database_path" "$AUTH_USER" "$ADMIN_PASSWORD" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1], timeout=10) as db:
    administrators = db.execute('select username,password_hash from administrators').fetchall()
    assert len(administrators) == 1, administrators
    username, password_hash = administrators[0]
    assert username == sys.argv[2]
    assert password_hash.startswith('$argon2id$v=19$m=19456,t=2,p=1$'), password_hash
    assert sys.argv[3] not in password_hash
    sessions = db.execute('select token_hash,csrf_hash,expires_at from admin_sessions').fetchall()
    assert sessions and all(len(token) == 64 and len(csrf) == 64 and expires > 0 for token, csrf, expires in sessions)
PY
[[ "$(stat -c '%s' "$master_key_path")" = 32 ]] || fail "storage master key is not exactly 32 bytes"
[[ "$(stat -c '%a' "$master_key_path")" = 600 ]] || fail "storage master key permissions are not 0600"

session_a_jar="$RUN_TMP_DIR/session-a.cookies"
cp "$COOKIE_JAR" "$session_a_jar"
chmod 600 "$session_a_jar"
first_session=$(session_token_from_jar)
login_admin
second_session=$(session_token_from_jar)
[[ -n "$first_session" && -n "$second_session" && "$first_session" != "$second_session" ]] \
  || fail "independent logins reused the same session token"
cross_session_csrf=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/cross-session-csrf.json" -w '%{http_code}' \
  -b "$session_a_jar" -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF_TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"cross-session-csrf-must-block"}' "$BASE/api/albums")
[[ "$cross_session_csrf" = 403 ]] || fail "Session B CSRF was accepted with Session A cookies (HTTP $cross_session_csrf)"
python3 - "$database_path" "$second_session" <<'PY'
import hashlib, sqlite3, sys
token_hash = hashlib.sha256(sys.argv[2].encode()).hexdigest()
with sqlite3.connect(sys.argv[1], timeout=10) as db:
    result = db.execute('update admin_sessions set expires_at=0 where token_hash=?', (token_hash,))
    db.commit()
    assert result.rowcount == 1, result.rowcount
PY
expired_session=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/expired-session.json" -w '%{http_code}' "$BASE/api/conversions")
[[ "$expired_session" = 401 ]] || fail "expired session returned HTTP $expired_session"
auth_curl -fsS "$BASE/api/auth/status" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["initialized"] and not value["authenticated"], value'
login_admin

repeat_registration=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/repeat-registration.json" -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"another-admin\",\"password\":\"$ADMIN_PASSWORD\"}" \
  "$BASE/api/auth/register")
[[ "$repeat_registration" = 409 ]] || fail "second administrator registration returned HTTP $repeat_registration"
old_token_code=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/old-token.json" -w '%{http_code}' \
  -H 'X-Admin-Token: obsolete-token' "$BASE/api/conversions")
[[ "$old_token_code" = 401 ]] || fail "legacy X-Admin-Token still authenticated (HTTP $old_token_code)"
missing_csrf=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/missing-csrf.json" -w '%{http_code}' \
  -b "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d '{"name":"csrf-must-block"}' "$BASE/api/albums")
[[ "$missing_csrf" = 403 ]] || fail "mutation without CSRF returned HTTP $missing_csrf"
wrong_csrf=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/wrong-csrf.json" -w '%{http_code}' \
  -b "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H 'X-CSRF-Token: wrong' \
  -H 'Content-Type: application/json' -d '{"name":"csrf-must-block"}' "$BASE/api/albums")
[[ "$wrong_csrf" = 403 ]] || fail "mutation with wrong CSRF returned HTTP $wrong_csrf"
curl -fsS "$BASE/api/albums" | python3 -c 'import json,sys; assert json.load(sys.stdin) == []'
auth_curl -sS -D "$RUN_TMP_DIR/preflight.headers" -o /dev/null -X OPTIONS \
  -H 'Origin: http://evil.invalid' -H 'Access-Control-Request-Method: POST' \
  -H 'Access-Control-Request-Headers: X-Requested-With,X-CSRF-Token,Content-Type' \
  "$BASE/api/albums"
if grep -qi '^access-control-allow-origin:' "$RUN_TMP_DIR/preflight.headers"; then fail "cross-origin preflight was allowed"; fi

# Original public site identity settings are database-backed and only administrators can change them.
curl -fsS "$BASE/api/settings/site" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value == {"title":"ChronoFrame","slogan":"Frame the moments that matter.","author":"ChronoFrame","avatarUrl":"/web-app-manifest-192x192.png","theme":"system"}, value'
unauthorized_site_settings=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/site-settings-unauthorized.json" -w '%{http_code}' \
  -X PUT -H 'Content-Type: application/json' \
  -d '{"title":"Forbidden","slogan":"","author":"","avatarUrl":"","theme":"system"}' "$BASE/api/settings/site")
[[ "$unauthorized_site_settings" = 401 ]] || fail "unauthenticated site settings PUT returned HTTP $unauthorized_site_settings"
invalid_site_settings=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/site-settings-invalid.json" -w '%{http_code}' \
  -X PUT -H 'Content-Type: application/json' \
  -d '{"title":"E2E Gallery","slogan":"Test slogan","author":"Test author","avatarUrl":"javascript:alert(1)","theme":"dark"}' "$BASE/api/settings/site")
[[ "$invalid_site_settings" = 400 ]] || fail "unsafe site avatar URL returned HTTP $invalid_site_settings"
api -X PUT -H 'Content-Type: application/json' \
  -d '{"title":"  E2E Gallery  ","slogan":"  Test slogan  ","author":"  Test author  ","avatarUrl":"https://example.com/avatar.png","theme":"dark"}' \
  "$BASE/api/settings/site" >"$RUN_TMP_DIR/site-settings.json"
curl -fsS "$BASE/api/settings/site" >"$RUN_TMP_DIR/site-settings-public.json"
python3 - "$RUN_TMP_DIR/site-settings.json" "$RUN_TMP_DIR/site-settings-public.json" <<'PY'
import json, sys
expected = {"title":"E2E Gallery","slogan":"Test slogan","author":"Test author","avatarUrl":"https://example.com/avatar.png","theme":"dark"}
for path in sys.argv[1:]:
    assert json.load(open(path)) == expected
PY
echo "PASS public site identity defaults, validation and administrator-only persistence"

# Album metadata is administrator-only. Dates stay date-only strings and descriptions are trimmed.
date_album_response=$(api -H 'Content-Type: application/json' -d '{"name":"E2E album metadata","description":"  Initial album story  "}' "$BASE/api/albums")
date_album=$(printf '%s' "$date_album_response" | json_value "['id']")
date_album_created_at=$(printf '%s' "$date_album_response" | json_value "['createdAt']")
printf '%s' "$date_album_response" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["description"] == "Initial album story", value; assert value["displayCreatedDate"] is None and value["photoDateStart"] is None and value["photoDateEnd"] is None, value'

unauthorized_album_patch=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-unauthorized.json" -w '%{http_code}' \
  -X PATCH -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2020-02-29"}' "$BASE/api/albums/$date_album")
[[ "$unauthorized_album_patch" = 401 ]] || fail "unauthenticated album date PATCH returned HTTP $unauthorized_album_patch"
missing_csrf_album_patch=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-missing-csrf.json" -w '%{http_code}' \
  -X PATCH -b "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2020-02-29"}' "$BASE/api/albums/$date_album")
[[ "$missing_csrf_album_patch" = 403 ]] || fail "album date PATCH without CSRF returned HTTP $missing_csrf_album_patch"
wrong_csrf_album_patch=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-wrong-csrf.json" -w '%{http_code}' \
  -X PATCH -b "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H 'X-CSRF-Token: wrong' \
  -H 'Content-Type: application/json' -d '{"displayCreatedDate":"2020-02-29"}' "$BASE/api/albums/$date_album")
[[ "$wrong_csrf_album_patch" = 403 ]] || fail "album date PATCH with wrong CSRF returned HTTP $wrong_csrf_album_patch"

partial_album_range=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-partial.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' -d '{"photoDateStart":"2019-01-01"}' "$BASE/api/albums/$date_album")
[[ "$partial_album_range" = 400 ]] || fail "one-sided album photo range returned HTTP $partial_album_range"
invalid_album_date=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-invalid.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' -d '{"displayCreatedDate":"2023-02-29"}' "$BASE/api/albums/$date_album")
[[ "$invalid_album_date" = 400 ]] || fail "invalid calendar date returned HTTP $invalid_album_date"
reversed_album_range=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-reversed.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"photoDateStart":"2022-12-31","photoDateEnd":"2022-01-01"}' "$BASE/api/albums/$date_album")
[[ "$reversed_album_range" = 400 ]] || fail "reversed album photo range returned HTTP $reversed_album_range"
missing_album_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-not-found.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' -d '{"displayCreatedDate":"2020-02-29"}' "$BASE/api/albums/does-not-exist")
[[ "$missing_album_patch" = 404 ]] || fail "unknown album date PATCH returned HTTP $missing_album_patch"

empty_album_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-empty.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' -d '{}' "$BASE/api/albums/$date_album")
[[ "$empty_album_patch" = 400 ]] || fail "empty album date PATCH returned HTTP $empty_album_patch"
unknown_album_field=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-unknown-field.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' -d '{"unknownDate":"2020-01-01"}' "$BASE/api/albums/$date_album")
[[ "$unknown_album_field" = 422 ]] || fail "unknown album date field returned HTTP $unknown_album_field"

initial_album_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-initial.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2020-02-29","photoDateStart":"2018-03-04","photoDateEnd":"2020-01-02"}' "$BASE/api/albums/$date_album")
[[ "$initial_album_patch" = 200 ]] || fail "initial valid album dates returned HTTP $initial_album_patch"
display_only_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-display-only.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2011-12-30"}' "$BASE/api/albums/$date_album")
[[ "$display_only_patch" = 200 ]] || fail "display-only album date PATCH returned HTTP $display_only_patch"
range_only_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-range-only.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"photoDateStart":"2001-01-01","photoDateEnd":"2002-02-02"}' "$BASE/api/albums/$date_album")
[[ "$range_only_patch" = 200 ]] || fail "range-only album date PATCH returned HTTP $range_only_patch"
clear_display_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-display-cleared.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"   "}' "$BASE/api/albums/$date_album")
[[ "$clear_display_patch" = 200 ]] || fail "display date clearing returned HTTP $clear_display_patch"
restore_display_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-display-restored.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2011-12-30"}' "$BASE/api/albums/$date_album")
[[ "$restore_display_patch" = 200 ]] || fail "display date restore returned HTTP $restore_display_patch"
clear_range_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-range-cleared.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"photoDateStart":"","photoDateEnd":null}' "$BASE/api/albums/$date_album")
[[ "$clear_range_patch" = 200 ]] || fail "photo date range clearing returned HTTP $clear_range_patch"
valid_album_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-date-valid.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"displayCreatedDate":"2011-12-30","photoDateStart":"2010-01-02","photoDateEnd":"2012-03-04"}' "$BASE/api/albums/$date_album")
[[ "$valid_album_patch" = 200 ]] || fail "final valid album dates returned HTTP $valid_album_patch"
description_patch=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-description-valid.json" -w '%{http_code}' \
  -X PATCH -H 'Content-Type: application/json' \
  -d '{"description":"  Updated album story\nSecond line  "}' "$BASE/api/albums/$date_album")
[[ "$description_patch" = 200 ]] || fail "valid album description returned HTTP $description_patch"
curl -fsS --connect-timeout 5 --max-time 30 "$BASE/api/albums" >"$RUN_TMP_DIR/album-date-list.json"
curl -fsS --connect-timeout 5 --max-time 30 "$BASE/api/albums/$date_album" >"$RUN_TMP_DIR/album-date-detail.json"
python3 - "$date_album" "$date_album_created_at" \
  "$RUN_TMP_DIR/album-date-initial.json" "$RUN_TMP_DIR/album-date-display-only.json" \
  "$RUN_TMP_DIR/album-date-range-only.json" "$RUN_TMP_DIR/album-date-display-cleared.json" \
  "$RUN_TMP_DIR/album-date-display-restored.json" "$RUN_TMP_DIR/album-date-range-cleared.json" \
  "$RUN_TMP_DIR/album-date-valid.json" "$RUN_TMP_DIR/album-date-list.json" "$RUN_TMP_DIR/album-date-detail.json" <<'PY'
import json, sys
album_id, created_at = sys.argv[1], int(sys.argv[2])
states = [json.load(open(path)) for path in sys.argv[3:10]]
expected = [
    ("2020-02-29", "2018-03-04", "2020-01-02"),
    ("2011-12-30", "2018-03-04", "2020-01-02"),
    ("2011-12-30", "2001-01-01", "2002-02-02"),
    (None, "2001-01-01", "2002-02-02"),
    ("2011-12-30", "2001-01-01", "2002-02-02"),
    ("2011-12-30", None, None),
    ("2011-12-30", "2010-01-02", "2012-03-04"),
]
for value, dates in zip(states, expected, strict=True):
    assert value["id"] == album_id, value
    assert value["createdAt"] == created_at, value
    assert (value["displayCreatedDate"], value["photoDateStart"], value["photoDateEnd"]) == dates, value
listed = next(album for album in json.load(open(sys.argv[10])) if album["id"] == album_id)
detail = json.load(open(sys.argv[11]))
for value in (listed, detail):
    assert value["id"] == album_id, value
    assert value["createdAt"] == created_at, value
    assert value["description"] == "Updated album story\nSecond line", value
    assert value["displayCreatedDate"] == "2011-12-30", value
    assert value["photoDateStart"] == "2010-01-02", value
    assert value["photoDateEnd"] == "2012-03-04", value
assert detail["photos"] == [], detail
PY

order_album_response=$(api -H 'Content-Type: application/json' -d '{"name":"E2E reorder target","description":"Second album"}' "$BASE/api/albums")
order_album=$(printf '%s' "$order_album_response" | json_value "['id']")
api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$date_album\",\"$order_album\"]}" "$BASE/api/albums/order" >"$RUN_TMP_DIR/album-order.json"
python3 - "$date_album" "$order_album" "$RUN_TMP_DIR/album-order.json" <<'PY'
import json, sys
album_ids = [sys.argv[1], sys.argv[2]]
albums = json.load(open(sys.argv[3]))
assert [album["id"] for album in albums] == album_ids, albums
assert [album["position"] for album in albums] == [0, 1], albums
PY
duplicate_order=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/album-order-duplicate.json" -w '%{http_code}' \
  -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$date_album\",\"$date_album\"]}" "$BASE/api/albums/order")
[[ "$duplicate_order" = 400 ]] || fail "duplicate/incomplete album order returned HTTP $duplicate_order"
echo "PASS album metadata authorization, validation, description persistence, date updates and exact reordering"

logout_cookie=$(session_cookie_header)
auth_cookie_curl -fsS -D "$RUN_TMP_DIR/logout.headers" -X POST "$BASE/api/auth/logout" >"$RUN_TMP_DIR/logout.json"
python3 - "$RUN_TMP_DIR/logout.headers" <<'PY'
from pathlib import Path
import sys
headers = Path(sys.argv[1]).read_text().lower()
cookies = [line for line in headers.splitlines() if line.startswith('set-cookie: cf_')]
assert len(cookies) == 2, cookies
assert any(line.startswith('set-cookie: cf_session=') for line in cookies), cookies
assert any(line.startswith('set-cookie: cf_csrf=') for line in cookies), cookies
assert all('max-age=0' in line and 'path=/' in line for line in cookies), cookies
PY
# curl 8.x can retain an already loaded HttpOnly cookie in a Netscape jar even after
# receiving a valid Max-Age=0 deletion header. The headers above are the browser-facing
# contract; clear the test client's local copy, then verify server-side invalidation by replay.
clear_auth
auth_cookie_curl -fsS "$BASE/api/auth/status" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["initialized"] and not value["authenticated"], value'
logout_replay=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/logout-replay.json" -w '%{http_code}' \
  -H "Cookie: $logout_cookie" -H 'X-Requested-With: ChronoFrame' "$BASE/api/conversions")
[[ "$logout_replay" = 401 ]] || fail "logged-out session replay returned HTTP $logout_replay"
login_admin
echo "PASS Nuxt entrypoint, atomic first-admin registration, password login, Cookie session, CSRF and logout"

# Local storage and the JPEG alias.
settings "$LOCAL_SETTINGS" >/dev/null
batch_album=$(make_album "E2E atomic upload")
batch_code=$(auth_curl -sS --connect-timeout 5 --max-time 60 -o "$RUN_TMP_DIR/batch.json" -w '%{http_code}' -X POST \
  -F "files=@$SOURCE;filename=valid.png" -F "files=@$SOURCE;filename=invalid.jpeg" "$BASE/api/albums/$batch_album/photos")
[[ "$batch_code" = 400 ]] || fail "mixed invalid upload batch returned HTTP $batch_code"
api "$BASE/api/albums/$batch_album/photos" | python3 -c 'import json,sys; assert json.load(sys.stdin) == []'
missing_album_code=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/missing-album.json" -w '%{http_code}' -X POST \
  -F "files=@$SOURCE;filename=orphan.png" "$BASE/api/albums/does-not-exist/photos")
[[ "$missing_album_code" = 404 ]] || fail "upload without an album returned HTTP $missing_album_code"
local_webp_album=$(make_album "E2E local WEBP")
local_webp_upload=$(upload "$local_webp_album" "local.png")
local_png_source=$(printf '%s' "$local_webp_upload" | json_value "[0]['id']")
wrong_format=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/wrong-format.json" -w '%{http_code}' -X POST -F "files=@$SOURCE;filename=wrong.jpeg" "$BASE/api/albums/$local_webp_album/photos")
[[ "$wrong_format" = 400 ]] || fail "mislabelled image returned HTTP $wrong_format"
invalid_target=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/invalid-target.json" -w '%{http_code}' -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$local_webp_album\"],\"targetFormat\":\"gif\"}" "$BASE/api/conversions")
[[ "$invalid_target" = 400 ]] || fail "unsupported target returned HTTP $invalid_target"
local_webp_job=$(start_job "$local_webp_album" webp)
[[ "$(wait_job "$local_webp_job")" = completed ]]
assert_completed_job "$local_webp_job" 1 webp 'local.png'
local_webp_target=$(first_job_target "$local_webp_job")
cp "$RUN_TMP_DIR/output-$local_webp_job-0.webp" "$RUN_TMP_DIR/fixture.webp"
upload "$local_webp_album" "local.png" >/dev/null

unauthorized_export=$(curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/export-unauthorized.json" -w '%{http_code}' "$BASE/api/albums/export?albumIds=$local_webp_album")
[[ "$unauthorized_export" = 401 ]] || fail "unauthenticated album export returned HTTP $unauthorized_export"
api -D "$RUN_TMP_DIR/export-single.headers" "$BASE/api/albums/export?albumIds=$local_webp_album" -o "$RUN_TMP_DIR/export-single.zip"
python3 - "$RUN_TMP_DIR/export-single.zip" "$RUN_TMP_DIR/export-single.headers" <<'PY'
import pathlib, sys, zipfile
archive, headers = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]).read_text().lower()
with zipfile.ZipFile(archive) as value:
    assert sorted(value.namelist()) == ["local (2).png", "local.png", "local.webp"], value.namelist()
    assert all(not name.startswith(('/', '\\')) and '..' not in pathlib.PurePosixPath(name).parts for name in value.namelist())
assert 'content-type: application/zip' in headers
assert 'content-disposition: attachment;' in headers and "filename*=utf-8''e2e%20local%20webp.zip" in headers
PY
(auth_curl -fsS --connect-timeout 5 --max-time 180 --limit-rate 1024 \
  "$BASE/api/albums/export?albumIds=$local_webp_album" -o "$RUN_TMP_DIR/export-aborted.zip") &
aborted_export_pid=$!
for _ in $(seq 1 100); do
  [[ -s "$RUN_TMP_DIR/export-aborted.zip" ]] && break
  kill -0 "$aborted_export_pid" 2>/dev/null || break
  sleep 0.1
done
[[ -s "$RUN_TMP_DIR/export-aborted.zip" ]] || fail "throttled export did not start streaming before interruption test"
kill "$aborted_export_pid" 2>/dev/null || true
wait "$aborted_export_pid" 2>/dev/null || true
assert_no_export_temps
empty_export_album=$(make_album "E2E empty export")
api "$BASE/api/albums/export?albumIds=$empty_export_album" -o "$RUN_TMP_DIR/export-empty.zip"
python3 - "$RUN_TMP_DIR/export-empty.zip" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as value:
    assert value.namelist() == []
PY
assert_no_export_temps

local_jpg_album=$(make_album "E2E local JPEG alias")
local_jpg_upload=$(upload "$local_jpg_album" "alias-source.png")
local_jpg_source=$(printf '%s' "$local_jpg_upload" | json_value "[0]['id']")
local_jpg_job=$(start_job "$local_jpg_album" jpeg)
[[ "$(wait_job "$local_jpg_job")" = completed ]]
assert_completed_job "$local_jpg_job" 1 jpg 'alias-source.png'
local_jpg_target=$(first_job_target "$local_jpg_job")
cp "$RUN_TMP_DIR/output-$local_jpg_job-0.jpg" "$RUN_TMP_DIR/fixture.jpg"
assert_delete_sources "$local_jpg_job" 1
deleted_code=$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$local_jpg_source/file")
[[ "$deleted_code" = 404 ]] || fail "deleted local source still returned HTTP $deleted_code"
curl -fsS "$BASE/api/photos/$local_jpg_target/file" -o "$RUN_TMP_DIR/local-survivor.jpg"
assert_signature "$RUN_TMP_DIR/local-survivor.jpg" jpg
api "$BASE/api/albums/export?albumIds=$local_webp_album,$local_jpg_album" -o "$RUN_TMP_DIR/export-multiple.zip"
python3 - "$RUN_TMP_DIR/export-multiple.zip" <<'PY'
import io, sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as outer:
    assert sorted(outer.namelist()) == ["E2E local JPEG alias.zip", "E2E local WEBP.zip"], outer.namelist()
    for name in outer.namelist():
        with zipfile.ZipFile(io.BytesIO(outer.read(name))) as inner:
            assert inner.testzip() is None
            assert inner.namelist(), (name, inner.namelist())
PY
duplicate_export=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/export-duplicate.json" -w '%{http_code}' "$BASE/api/albums/export?albumIds=$local_webp_album,$local_webp_album")
[[ "$duplicate_export" = 400 ]] || fail "duplicate album export returned HTTP $duplicate_export"
assert_no_export_temps
repeat_delete=$(auth_curl -sS -o "$RUN_TMP_DIR/repeat-delete.json" -w '%{http_code}' -X DELETE "$BASE/api/conversions/$local_jpg_job/delete-sources")
[[ "$repeat_delete" = 400 ]] || fail "repeat source deletion returned HTTP $repeat_delete"
switch_code=$(auth_curl -sS -o "$RUN_TMP_DIR/switch.json" -w '%{http_code}' -H 'Content-Type: application/json' -X PUT -d "$WEBDAV_SETTINGS" "$BASE/api/settings/storage")
[[ "$switch_code" = 400 ]] || fail "unsafe non-empty storage switch returned HTTP $switch_code"
assert_no_local_temps
echo "PASS local storage, single/multi nested ZIP export, strict conversions, explicit deletion and storage switch guard"

# WebDAV rejects bad credentials before commit, then recovers and supports conversion/deletion.
reset_stack
bad_webdav=$(auth_curl -sS --connect-timeout 5 --max-time 90 -o "$RUN_TMP_DIR/webdav-bad.json" -w '%{http_code}' -X PUT -H 'Content-Type: application/json' -d "$WEBDAV_BAD_SETTINGS" "$BASE/api/settings/storage")
[[ "$bad_webdav" = 400 ]] || fail "bad WebDAV credentials returned HTTP $bad_webdav"
settings "$WEBDAV_SETTINGS" >/dev/null
assert_secret_hidden 'e2e-webdav-password' webdavPasswordSet
assert_secret_encrypted storage_webdav_password 'e2e-webdav-password'
"${COMPOSE[@]}" restart chronoframe >/dev/null
wait_app_ready
assert_secret_hidden 'e2e-webdav-password' webdavPasswordSet
webdav_album=$(make_album "E2E WebDAV")
webdav_upload=$(upload "$webdav_album" "webdav.png")
webdav_source=$(printf '%s' "$webdav_upload" | json_value "[0]['id']")
webdav_job=$(start_job "$webdav_album" webp)
[[ "$(wait_job "$webdav_job")" = completed ]]
assert_completed_job "$webdav_job" 1 webp 'webdav.png'
webdav_target=$(first_job_target "$webdav_job")
curl -fsS "$BASE/api/photos/$webdav_source/file" -o "$RUN_TMP_DIR/webdav-source.png"
assert_signature "$RUN_TMP_DIR/webdav-source.png" png
assert_delete_sources "$webdav_job" 1
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$webdav_source/file")" = 404 ]]
curl -fsS "$BASE/api/photos/$webdav_target/file" -o "$RUN_TMP_DIR/webdav-target.webp"
assert_signature "$RUN_TMP_DIR/webdav-target.webp" webp
api "$BASE/api/albums/export?albumIds=$webdav_album" -o "$RUN_TMP_DIR/webdav-export.zip"
python3 - "$RUN_TMP_DIR/webdav-export.zip" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as value:
    assert value.testzip() is None and value.namelist() == ["webdav.webp"], value.namelist()
PY
assert_no_export_temps
webdav_container=$("${COMPOSE[@]}" ps -q webdav)
webdav_orphan=$(docker exec "$webdav_container" find /var/lib/dav -type f -name "$webdav_source.png" -print)
[[ -z "$webdav_orphan" ]] || fail "deleted WebDAV source object remains"
assert_no_webdav_temps
echo "PASS WebDAV credential rejection/recovery, encrypted settings, output retrieval and source deletion"

# S3/MinIO rejects bad credentials, survives an outage, and deletes only administrator-confirmed sources.
reset_stack
bad_s3=$(auth_curl -sS --connect-timeout 5 --max-time 90 -o "$RUN_TMP_DIR/s3-bad.json" -w '%{http_code}' -X PUT -H 'Content-Type: application/json' -d "$S3_BAD_SETTINGS" "$BASE/api/settings/storage")
[[ "$bad_s3" = 400 ]] || fail "bad S3 credentials returned HTTP $bad_s3"
settings "$S3_SETTINGS" >/dev/null
assert_secret_hidden 'e2e-minio-secret-change-me' s3SecretKeySet
assert_secret_encrypted storage_s3_secret_key 'e2e-minio-secret-change-me'
s3_album=$(make_album "E2E S3")
s3_upload=$(upload "$s3_album" "s3.png")
s3_source=$(printf '%s' "$s3_upload" | json_value "[0]['id']")
s3_source_key=$(printf '%s' "$s3_upload" | json_value "[0]['storageKey']")
s3_job=$(start_job "$s3_album" jpg)
[[ "$(wait_job "$s3_job")" = completed ]]
assert_completed_job "$s3_job" 1 jpg 's3.png'
s3_target=$(first_job_target "$s3_job")
"${COMPOSE[@]}" stop minio >/dev/null
outage_code=$(curl -sS --connect-timeout 5 --max-time 50 -o "$RUN_TMP_DIR/s3-outage.json" -w '%{http_code}' "$BASE/api/photos/$s3_target/file" || true)
[[ "$outage_code" = 500 ]] || fail "S3 outage returned HTTP ${outage_code:-curl-error}"
"${COMPOSE[@]}" start minio >/dev/null
wait_minio_ready
curl -fsS --connect-timeout 5 --max-time 60 "$BASE/api/photos/$s3_target/file" -o "$RUN_TMP_DIR/s3-recovered.jpg"
assert_signature "$RUN_TMP_DIR/s3-recovered.jpg" jpg
assert_delete_sources "$s3_job" 1
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$s3_source/file")" = 404 ]]
curl -fsS "$BASE/api/photos/$s3_target/file" -o "$RUN_TMP_DIR/s3-target.jpg"
assert_signature "$RUN_TMP_DIR/s3-target.jpg" jpg
api "$BASE/api/albums/export?albumIds=$s3_album" -o "$RUN_TMP_DIR/s3-export.zip"
python3 - "$RUN_TMP_DIR/s3-export.zip" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as value:
    assert value.testzip() is None and value.namelist() == ["s3.jpg"], value.namelist()
PY
assert_no_export_temps
s3_objects=$(docker run --rm --network "$NETWORK_NAME" --entrypoint /bin/sh minio/mc:latest -c \
  'mc alias set e2e http://minio:9000 e2e-minio-access e2e-minio-secret-change-me >/dev/null && mc find e2e/chronoframe-e2e/e2e')
if printf '%s\n' "$s3_objects" | grep -Fq "$s3_source_key"; then fail "deleted S3 source object remains"; fi
assert_no_s3_temps
echo "PASS S3 credential rejection/recovery, outage recovery, conversion retrieval and source deletion"

# Four input spellings against four output choices; JPEG is intentionally normalized to JPG.
for target in png jpg jpeg webp; do
  matrix_album=$(make_album "E2E matrix $target")
  upload_file "$matrix_album" "$SOURCE" input.png >/dev/null
  upload_file "$matrix_album" "$RUN_TMP_DIR/fixture.jpg" input.jpg >/dev/null
  upload_file "$matrix_album" "$RUN_TMP_DIR/fixture.jpg" input.jpeg >/dev/null
  upload_file "$matrix_album" "$RUN_TMP_DIR/fixture.webp" input.webp >/dev/null
  case "$target" in
    png) expected_total=3; canonical=png; names='input.jpg|input.jpeg|input.webp' ;;
    jpg|jpeg) expected_total=2; canonical=jpg; names='input.png|input.webp' ;;
    webp) expected_total=3; canonical=webp; names='input.png|input.jpg|input.jpeg' ;;
  esac
  matrix_job=$(start_job "$matrix_album" "$target")
  [[ "$(wait_job "$matrix_job")" = completed ]]
  assert_completed_job "$matrix_job" "$expected_total" "$canonical" "$names"
done

multi_a=$(make_album "E2E multi A")
multi_b=$(make_album "E2E multi B")
upload_file "$multi_a" "$SOURCE" multi-a-1.png >/dev/null
upload_file "$multi_a" "$RUN_TMP_DIR/fixture.jpg" multi-a-2.jpg >/dev/null
upload_file "$multi_b" "$SOURCE" multi-b-1.png >/dev/null
multi_job=$(start_multi_job "$multi_a" "$multi_b" webp)
[[ "$(wait_job "$multi_job")" = completed ]]
assert_completed_job "$multi_job" 3 webp 'multi-a-1.png|multi-a-2.jpg|multi-b-1.png'
assert_no_s3_temps
echo "PASS full PNG/JPG/JPEG/WEBP matrix and exact two-album selection"

# Concurrent jobs, writes and reads during conversion; cancellation must expose real intermediate progress.
reset_stack
settings "$LOCAL_SETTINGS" >/dev/null
python3 "$ROOT/scripts/make-fixture.py" "$LARGE_SOURCE" --width 1536 --height 1024
assert_signature "$LARGE_SOURCE" png
fixture_size=$(stat -c '%s' "$LARGE_SOURCE")
(( fixture_size >= 4000000 && fixture_size <= 10000000 )) || fail "large fixture has unsafe/unexpected size $fixture_size"
read_album=$(make_album "E2E concurrent reads")
read_upload=$(upload "$read_album" "read.png")
read_photo=$(printf '%s' "$read_upload" | json_value "[0]['id']")
stress_album=$(make_album "E2E cancellation")
parts=()
for i in $(seq 1 64); do parts+=(-F "files=@$LARGE_SOURCE;filename=stress-$i.png"); done
auth_curl -fsS --connect-timeout 5 --max-time 300 -X POST "${parts[@]}" "$BASE/api/albums/$stress_album/photos" >/dev/null
cancel_job=$(start_job "$stress_album" webp)
parallel_job=$(start_job "$stress_album" jpg)
wait_for_running "$cancel_job" 30
wait_for_running "$parallel_job" 30
python3 "$ROOT/scripts/vps-load.py" --base "$BASE" --requests 1600 --concurrency 32 --timeout 30 --max-p95-ms 5000 \
  --album-id "$read_album" --photo-id "$read_photo" --job-id "$cancel_job" \
  --cookie-jar "$COOKIE_JAR" >"$RUN_TMP_DIR/mixed-load.json" &
LOAD_PID=$!
write_album=$(make_album "E2E write during conversion")
upload "$write_album" "concurrent-write.png" >/dev/null
partial_completed=$(wait_for_partial_progress "$cancel_job" 180)
cancel_http=$(auth_curl -sS --connect-timeout 5 --max-time 30 -o "$RUN_TMP_DIR/cancel.json" -w '%{http_code}' -X POST "$BASE/api/conversions/$cancel_job/cancel")
[[ "$cancel_http" = 202 ]] || fail "cancel returned HTTP $cancel_http"
[[ "$(wait_job "$cancel_job" 300)" = cancelled ]]
assert_cancelled_job "$cancel_job" 64 "$partial_completed"
assert_sources_retained "$cancel_job" "$stress_album" 64
[[ "$(wait_job "$parallel_job" 600)" = completed ]]
assert_completed_job "$parallel_job" 64 jpg
if ! wait "$LOAD_PID"; then cat "$RUN_TMP_DIR/mixed-load.json" >&2 || true; fail "mixed read load failed"; fi
LOAD_PID=""
cat "$RUN_TMP_DIR/mixed-load.json"
assert_no_local_temps
echo "PASS four-worker progress, concurrent jobs/write/read load, monotonic cancellation and source retention"

# A hard kill after observed partial progress must recover deterministically without deleting originals or leaving temps.
reset_stack
settings "$LOCAL_SETTINGS" >/dev/null
restart_album=$(make_album "E2E hard restart")
auth_curl -fsS --connect-timeout 5 --max-time 300 -X POST "${parts[@]}" "$BASE/api/albums/$restart_album/photos" >/dev/null
restart_job=$(start_job "$restart_album" webp)
wait_for_running "$restart_job" 30
restart_completed=$(wait_for_partial_progress "$restart_job" 180)
"${COMPOSE[@]}" kill -s SIGKILL chronoframe >/dev/null
"${COMPOSE[@]}" up -d chronoframe >/dev/null
wait_app_ready
[[ "$(job_status "$restart_job")" = interrupted ]] || fail "hard-killed job was not marked interrupted"
assert_interrupted_job "$restart_job" 64 "$restart_completed"
assert_sources_retained "$restart_job" "$restart_album" 64
assert_no_local_temps
"${COMPOSE[@]}" logs --tail=200 chronoframe >"$RUN_TMP_DIR/final-chronoframe.log" 2>&1
if grep -qiE 'panic|fatal' "$RUN_TMP_DIR/final-chronoframe.log"; then fail "panic/fatal found in application logs"; fi
echo "PASS hard-kill recovery, persisted progress, source retention and temporary-object cleanup"

echo "ALL_E2E_TESTS_PASSED"
