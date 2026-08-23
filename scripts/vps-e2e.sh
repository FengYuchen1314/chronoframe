#!/usr/bin/env bash
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8188}"
TOKEN="${TOKEN:-e2e-admin-token-change-before-real-use}"
ROOT="${ROOT:-/opt/chronoframe-e2e/app}"
SOURCE="${SOURCE:-$ROOT/public/favicon-96x96.png}"
LARGE_SOURCE="${LARGE_SOURCE:-/tmp/e2e-large-fixture.png}"
PROJECT_NAME="${PROJECT_NAME:-app}"
NETWORK_NAME="${PROJECT_NAME}_default"
HDR=(-H "X-Admin-Token: $TOKEN")
COMPOSE=(docker compose --project-name "$PROJECT_NAME" -f "$ROOT/docker-compose.e2e.yml")
if [[ -n "${COMPOSE_OVERRIDE:-}" ]]; then COMPOSE+=(-f "$COMPOSE_OVERRIDE"); fi
LOAD_PID=""

fail() { echo "FAIL: $*" >&2; exit 1; }
cleanup() {
  if [[ -n "$LOAD_PID" ]] && kill -0 "$LOAD_PID" 2>/dev/null; then kill "$LOAD_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

json_value() { python3 -c "import json,sys; value=json.load(sys.stdin)$1; print(value)"; }
api() { curl -fsS --connect-timeout 5 --max-time 180 "${HDR[@]}" "$@"; }
make_album() { api -H 'Content-Type: application/json' -d "{\"name\":\"$1\"}" "$BASE/api/albums" | json_value "['id']"; }
upload() { api -X POST -F "files=@$SOURCE;filename=$2" "$BASE/api/albums/$1/photos"; }
upload_file() { api -X POST -F "files=@$2;filename=$3" "$BASE/api/albums/$1/photos"; }
start_job() { api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$1\"],\"targetFormat\":\"$2\"}" "$BASE/api/conversions" | json_value "['id']"; }
start_multi_job() { api -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$1\",\"$2\"],\"targetFormat\":\"$3\"}" "$BASE/api/conversions" | json_value "['id']"; }
job_status() { api "$BASE/api/conversions/$1?items=false" | json_value "['job']['status']"; }
settings() { api -X PUT -H 'Content-Type: application/json' -d "$1" "$BASE/api/settings/storage"; }

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
  [[ -d "$ROOT" ]] || fail "isolated E2E directory is missing"
  cd "$ROOT"
  [[ "$(pwd -P)" = "$ROOT" ]] || fail "refusing to reset outside $ROOT"
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null
  "${COMPOSE[@]}" up -d >/dev/null
  wait_app_ready
  wait_webdav_ready
  ensure_test_bucket
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
  local job=$1 expected_total=$2 expected_format=$3 expected_names=${4:--} snapshot_file="/tmp/e2e-job-$1.json"
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
    output="/tmp/e2e-output-$job-$index.$expected_format"
    curl -fsS --connect-timeout 5 --max-time 180 "$BASE/api/photos/$target_id/file" -o "$output"
    assert_signature "$output" "$expected_format"
    index=$((index + 1))
  done
}

first_job_target() {
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["items"][0]["targetPhotoId"])' "/tmp/e2e-job-$1.json"
}

assert_cancelled_job() {
  local job=$1 expected_total=$2 minimum_completed=$3 snapshot_file="/tmp/e2e-job-$1.json"
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
  local job=$1 expected_total=$2 minimum_completed=$3 snapshot_file="/tmp/e2e-job-$1.json"
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
    output="/tmp/e2e-interrupted-$job-$index.webp"
    curl -fsS --connect-timeout 5 --max-time 180 "$BASE/api/photos/$target_id/file" -o "$output"
    assert_signature "$output" webp
    index=$((index + 1))
  done
}

assert_sources_retained() {
  local job=$1 album=$2 expected=$3 job_file="/tmp/e2e-job-$1.json" photos_file="/tmp/e2e-photos-$1.json"
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
    output="/tmp/e2e-retained-$job-$index.png"
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

LOCAL_SETTINGS='{"backend":"local","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
WEBDAV_SETTINGS='{"backend":"webdav","localPath":"/app/data/e2e-local","webdavUrl":"http://webdav/","webdavUsername":"e2e-webdav-user","webdavPassword":"e2e-webdav-password","webdavPrefix":"e2e","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
WEBDAV_BAD_SETTINGS='{"backend":"webdav","localPath":"/app/data/e2e-local","webdavUrl":"http://webdav/","webdavUsername":"e2e-webdav-user","webdavPassword":"definitely-wrong","webdavPrefix":"e2e","s3Endpoint":"","s3Region":"us-east-1","s3Bucket":"","s3AccessKey":"","s3Prefix":"chronoframe"}'
S3_SETTINGS='{"backend":"s3","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"http://minio:9000","s3Region":"us-east-1","s3Bucket":"chronoframe-e2e","s3AccessKey":"e2e-minio-access","s3SecretKey":"e2e-minio-secret-change-me","s3Prefix":"e2e"}'
S3_BAD_SETTINGS='{"backend":"s3","localPath":"/app/data/e2e-local","webdavUrl":"","webdavUsername":"","webdavPrefix":"chronoframe","s3Endpoint":"http://minio:9000","s3Region":"us-east-1","s3Bucket":"chronoframe-e2e","s3AccessKey":"e2e-minio-access","s3SecretKey":"definitely-wrong","s3Prefix":"e2e"}'

# Start from clean, project-scoped Docker volumes and prove the static Nuxt app plus auth boundary.
reset_stack
curl -fsS --connect-timeout 5 --max-time 30 "$BASE/" | grep -q 'id="__nuxt"'
unauthorized=$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/e2e-unauthorized.json -w '%{http_code}' -H 'Content-Type: application/json' -d '{"name":"forbidden"}' "$BASE/api/albums")
[[ "$unauthorized" = 401 ]] || fail "unauthenticated mutation returned HTTP $unauthorized"
echo "PASS Nuxt entrypoint and mutation authentication"

# Local storage and the JPEG alias.
settings "$LOCAL_SETTINGS" >/dev/null
batch_album=$(make_album "E2E atomic upload")
batch_code=$(curl -sS --connect-timeout 5 --max-time 60 -o /tmp/e2e-batch.json -w '%{http_code}' "${HDR[@]}" -X POST \
  -F "files=@$SOURCE;filename=valid.png" -F "files=@$SOURCE;filename=invalid.jpeg" "$BASE/api/albums/$batch_album/photos")
[[ "$batch_code" = 400 ]] || fail "mixed invalid upload batch returned HTTP $batch_code"
api "$BASE/api/albums/$batch_album/photos" | python3 -c 'import json,sys; assert json.load(sys.stdin) == []'
missing_album_code=$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/e2e-missing-album.json -w '%{http_code}' "${HDR[@]}" -X POST \
  -F "files=@$SOURCE;filename=orphan.png" "$BASE/api/albums/does-not-exist/photos")
[[ "$missing_album_code" = 404 ]] || fail "upload without an album returned HTTP $missing_album_code"
local_webp_album=$(make_album "E2E local WEBP")
local_webp_upload=$(upload "$local_webp_album" "local.png")
local_png_source=$(printf '%s' "$local_webp_upload" | json_value "[0]['id']")
wrong_format=$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/e2e-wrong-format.json -w '%{http_code}' "${HDR[@]}" -X POST -F "files=@$SOURCE;filename=wrong.jpeg" "$BASE/api/albums/$local_webp_album/photos")
[[ "$wrong_format" = 400 ]] || fail "mislabelled image returned HTTP $wrong_format"
invalid_target=$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/e2e-invalid-target.json -w '%{http_code}' "${HDR[@]}" -H 'Content-Type: application/json' -d "{\"albumIds\":[\"$local_webp_album\"],\"targetFormat\":\"gif\"}" "$BASE/api/conversions")
[[ "$invalid_target" = 400 ]] || fail "unsupported target returned HTTP $invalid_target"
local_webp_job=$(start_job "$local_webp_album" webp)
[[ "$(wait_job "$local_webp_job")" = completed ]]
assert_completed_job "$local_webp_job" 1 webp 'local.png'
local_webp_target=$(first_job_target "$local_webp_job")
cp "/tmp/e2e-output-$local_webp_job-0.webp" /tmp/e2e-fixture.webp

local_jpg_album=$(make_album "E2E local JPEG alias")
local_jpg_upload=$(upload "$local_jpg_album" "alias-source.png")
local_jpg_source=$(printf '%s' "$local_jpg_upload" | json_value "[0]['id']")
local_jpg_job=$(start_job "$local_jpg_album" jpeg)
[[ "$(wait_job "$local_jpg_job")" = completed ]]
assert_completed_job "$local_jpg_job" 1 jpg 'alias-source.png'
local_jpg_target=$(first_job_target "$local_jpg_job")
cp "/tmp/e2e-output-$local_jpg_job-0.jpg" /tmp/e2e-fixture.jpg
assert_delete_sources "$local_jpg_job" 1
deleted_code=$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$local_jpg_source/file")
[[ "$deleted_code" = 404 ]] || fail "deleted local source still returned HTTP $deleted_code"
curl -fsS "$BASE/api/photos/$local_jpg_target/file" -o /tmp/e2e-local-survivor.jpg
assert_signature /tmp/e2e-local-survivor.jpg jpg
repeat_delete=$(curl -sS -o /tmp/e2e-repeat-delete.json -w '%{http_code}' "${HDR[@]}" -X DELETE "$BASE/api/conversions/$local_jpg_job/delete-sources")
[[ "$repeat_delete" = 400 ]] || fail "repeat source deletion returned HTTP $repeat_delete"
switch_code=$(curl -sS -o /tmp/e2e-switch.json -w '%{http_code}' "${HDR[@]}" -H 'Content-Type: application/json' -X PUT -d "$WEBDAV_SETTINGS" "$BASE/api/settings/storage")
[[ "$switch_code" = 400 ]] || fail "unsafe non-empty storage switch returned HTTP $switch_code"
assert_no_local_temps
echo "PASS local storage, strict conversions, explicit deletion and storage switch guard"

# WebDAV rejects bad credentials before commit, then recovers and supports conversion/deletion.
reset_stack
bad_webdav=$(curl -sS --connect-timeout 5 --max-time 90 -o /tmp/e2e-webdav-bad.json -w '%{http_code}' "${HDR[@]}" -X PUT -H 'Content-Type: application/json' -d "$WEBDAV_BAD_SETTINGS" "$BASE/api/settings/storage")
[[ "$bad_webdav" = 400 ]] || fail "bad WebDAV credentials returned HTTP $bad_webdav"
settings "$WEBDAV_SETTINGS" >/dev/null
assert_secret_hidden 'e2e-webdav-password' webdavPasswordSet
webdav_album=$(make_album "E2E WebDAV")
webdav_upload=$(upload "$webdav_album" "webdav.png")
webdav_source=$(printf '%s' "$webdav_upload" | json_value "[0]['id']")
webdav_job=$(start_job "$webdav_album" webp)
[[ "$(wait_job "$webdav_job")" = completed ]]
assert_completed_job "$webdav_job" 1 webp 'webdav.png'
webdav_target=$(first_job_target "$webdav_job")
curl -fsS "$BASE/api/photos/$webdav_source/file" -o /tmp/e2e-webdav-source.png
assert_signature /tmp/e2e-webdav-source.png png
assert_delete_sources "$webdav_job" 1
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$webdav_source/file")" = 404 ]]
curl -fsS "$BASE/api/photos/$webdav_target/file" -o /tmp/e2e-webdav-target.webp
assert_signature /tmp/e2e-webdav-target.webp webp
webdav_container=$("${COMPOSE[@]}" ps -q webdav)
webdav_orphan=$(docker exec "$webdav_container" find /var/lib/dav -type f -name "$webdav_source.png" -print)
[[ -z "$webdav_orphan" ]] || fail "deleted WebDAV source object remains"
assert_no_webdav_temps
echo "PASS WebDAV credential rejection/recovery, encrypted settings, output retrieval and source deletion"

# S3/MinIO rejects bad credentials, survives an outage, and deletes only administrator-confirmed sources.
reset_stack
bad_s3=$(curl -sS --connect-timeout 5 --max-time 90 -o /tmp/e2e-s3-bad.json -w '%{http_code}' "${HDR[@]}" -X PUT -H 'Content-Type: application/json' -d "$S3_BAD_SETTINGS" "$BASE/api/settings/storage")
[[ "$bad_s3" = 400 ]] || fail "bad S3 credentials returned HTTP $bad_s3"
settings "$S3_SETTINGS" >/dev/null
assert_secret_hidden 'e2e-minio-secret-change-me' s3SecretKeySet
s3_album=$(make_album "E2E S3")
s3_upload=$(upload "$s3_album" "s3.png")
s3_source=$(printf '%s' "$s3_upload" | json_value "[0]['id']")
s3_source_key=$(printf '%s' "$s3_upload" | json_value "[0]['storageKey']")
s3_job=$(start_job "$s3_album" jpg)
[[ "$(wait_job "$s3_job")" = completed ]]
assert_completed_job "$s3_job" 1 jpg 's3.png'
s3_target=$(first_job_target "$s3_job")
"${COMPOSE[@]}" stop minio >/dev/null
outage_code=$(curl -sS --connect-timeout 5 --max-time 50 -o /tmp/e2e-s3-outage.json -w '%{http_code}' "$BASE/api/photos/$s3_target/file" || true)
[[ "$outage_code" = 500 ]] || fail "S3 outage returned HTTP ${outage_code:-curl-error}"
"${COMPOSE[@]}" start minio >/dev/null
wait_minio_ready
curl -fsS --connect-timeout 5 --max-time 60 "$BASE/api/photos/$s3_target/file" -o /tmp/e2e-s3-recovered.jpg
assert_signature /tmp/e2e-s3-recovered.jpg jpg
assert_delete_sources "$s3_job" 1
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/photos/$s3_source/file")" = 404 ]]
curl -fsS "$BASE/api/photos/$s3_target/file" -o /tmp/e2e-s3-target.jpg
assert_signature /tmp/e2e-s3-target.jpg jpg
s3_objects=$(docker run --rm --network "$NETWORK_NAME" --entrypoint /bin/sh minio/mc:latest -c \
  'mc alias set e2e http://minio:9000 e2e-minio-access e2e-minio-secret-change-me >/dev/null && mc find e2e/chronoframe-e2e/e2e')
if printf '%s\n' "$s3_objects" | grep -Fq "$s3_source_key"; then fail "deleted S3 source object remains"; fi
assert_no_s3_temps
echo "PASS S3 credential rejection/recovery, outage recovery, conversion retrieval and source deletion"

# Four input spellings against four output choices; JPEG is intentionally normalized to JPG.
for target in png jpg jpeg webp; do
  matrix_album=$(make_album "E2E matrix $target")
  upload_file "$matrix_album" "$SOURCE" input.png >/dev/null
  upload_file "$matrix_album" /tmp/e2e-fixture.jpg input.jpg >/dev/null
  upload_file "$matrix_album" /tmp/e2e-fixture.jpg input.jpeg >/dev/null
  upload_file "$matrix_album" /tmp/e2e-fixture.webp input.webp >/dev/null
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
upload_file "$multi_a" /tmp/e2e-fixture.jpg multi-a-2.jpg >/dev/null
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
curl -fsS --connect-timeout 5 --max-time 300 "${HDR[@]}" -X POST "${parts[@]}" "$BASE/api/albums/$stress_album/photos" >/dev/null
cancel_job=$(start_job "$stress_album" webp)
parallel_job=$(start_job "$stress_album" jpg)
wait_for_running "$cancel_job" 30
wait_for_running "$parallel_job" 30
python3 "$ROOT/scripts/vps-load.py" --base "$BASE" --requests 1600 --concurrency 32 --timeout 30 --max-p95-ms 5000 \
  --album-id "$read_album" --photo-id "$read_photo" --job-id "$cancel_job" --admin-token "$TOKEN" >/tmp/e2e-mixed-load.json &
LOAD_PID=$!
write_album=$(make_album "E2E write during conversion")
upload "$write_album" "concurrent-write.png" >/dev/null
partial_completed=$(wait_for_partial_progress "$cancel_job" 180)
cancel_http=$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/e2e-cancel.json -w '%{http_code}' "${HDR[@]}" -X POST "$BASE/api/conversions/$cancel_job/cancel")
[[ "$cancel_http" = 202 ]] || fail "cancel returned HTTP $cancel_http"
[[ "$(wait_job "$cancel_job" 300)" = cancelled ]]
assert_cancelled_job "$cancel_job" 64 "$partial_completed"
assert_sources_retained "$cancel_job" "$stress_album" 64
[[ "$(wait_job "$parallel_job" 600)" = completed ]]
assert_completed_job "$parallel_job" 64 jpg
if ! wait "$LOAD_PID"; then cat /tmp/e2e-mixed-load.json >&2 || true; fail "mixed read load failed"; fi
LOAD_PID=""
cat /tmp/e2e-mixed-load.json
assert_no_local_temps
echo "PASS four-worker progress, concurrent jobs/write/read load, monotonic cancellation and source retention"

# A hard kill after observed partial progress must recover deterministically without deleting originals or leaving temps.
reset_stack
settings "$LOCAL_SETTINGS" >/dev/null
restart_album=$(make_album "E2E hard restart")
curl -fsS --connect-timeout 5 --max-time 300 "${HDR[@]}" -X POST "${parts[@]}" "$BASE/api/albums/$restart_album/photos" >/dev/null
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
"${COMPOSE[@]}" logs --tail=200 chronoframe 2>&1 | grep -qiE 'panic|fatal' && fail "panic/fatal found in application logs"
echo "PASS hard-kill recovery, persisted progress, source retention and temporary-object cleanup"

echo "ALL_E2E_TESTS_PASSED"
