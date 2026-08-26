#!/usr/bin/env bash
set -euo pipefail

APP_CONTAINER="chronoframe-s3-cleanup-e2e-app"
MINIO_CONTAINER="chronoframe-s3-cleanup-e2e-minio"
MC_CONTAINER="chronoframe-s3-cleanup-e2e-mc"
NETWORK="chronoframe-s3-cleanup-e2e-network"
DATA_VOLUME="chronoframe-s3-cleanup-e2e-data"
IMAGE="${CHRONOFRAME_IMAGE:-chronoframe:s3-cleanup-test}"
BASE="http://127.0.0.1:8299"
BUCKET="chronoframe-cleanup-e2e"
PREFIX="chronoframe-e2e"
RUN_DIR=$(mktemp -d "${TMPDIR:-/tmp}/chronoframe-s3-cleanup.XXXXXX")
COOKIE_JAR="$RUN_DIR/cookies"

cleanup_resources() {
  docker rm -f "$APP_CONTAINER" "$MC_CONTAINER" "$MINIO_CONTAINER" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
  docker volume rm "$DATA_VOLUME" >/dev/null 2>&1 || true
}
cleanup() {
  cleanup_resources
  rm -rf -- "$RUN_DIR"
}
trap cleanup EXIT

cleanup_resources
docker network create "$NETWORK" >/dev/null
docker volume create "$DATA_VOLUME" >/dev/null
docker run -d --name "$MINIO_CONTAINER" --network "$NETWORK" \
  -e MINIO_ROOT_USER=e2e-minio-access \
  -e MINIO_ROOT_PASSWORD=e2e-minio-secret-change-me \
  minio/minio:latest server /data >/dev/null

for _ in $(seq 1 60); do
  if docker run --rm --network "$NETWORK" curlimages/curl:latest -fsS \
    "http://$MINIO_CONTAINER:9000/minio/health/ready" >/dev/null 2>&1; then break; fi
  sleep 1
done
docker run -d --name "$MC_CONTAINER" --network "$NETWORK" --entrypoint /bin/sh minio/mc:latest -c 'sleep 3600' >/dev/null
docker exec "$MC_CONTAINER" mc alias set e2e "http://$MINIO_CONTAINER:9000" e2e-minio-access e2e-minio-secret-change-me >/dev/null
docker exec "$MC_CONTAINER" mc mb --ignore-existing "e2e/$BUCKET" >/dev/null

docker run -d --name "$APP_CONTAINER" --network "$NETWORK" -p 127.0.0.1:8299:8080 \
  -e CF_DATABASE_URL=sqlite:///app/data/chronoframe.db?mode=rwc \
  -e CF_WEB_DIR=/app/web \
  -e CF_COOKIE_SECURE=false \
  -v "$DATA_VOLUME:/app/data" \
  "$IMAGE" >/dev/null
for _ in $(seq 1 90); do
  if curl -fsS "$BASE/api/albums" >/dev/null 2>&1; then break; fi
  sleep 1
done
curl -fsS -c "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H 'Content-Type: application/json' \
  -d '{"username":"cleanup-admin","password":"Cleanup-e2e-password-2026!"}' \
  "$BASE/api/auth/register" >/dev/null
CSRF=$(awk '$6 == "cf_csrf" { value=$7 } END { print value }' "$COOKIE_JAR")
api() {
  curl -fsS -b "$COOKIE_JAR" -H 'X-Requested-With: ChronoFrame' -H "X-CSRF-Token: $CSRF" "$@"
}

api -X PUT -H 'Content-Type: application/json' -d "{\"backend\":\"s3\",\"s3Endpoint\":\"http://$MINIO_CONTAINER:9000\",\"s3Region\":\"us-east-1\",\"s3Bucket\":\"$BUCKET\",\"s3AccessKey\":\"e2e-minio-access\",\"s3SecretKey\":\"e2e-minio-secret-change-me\",\"s3Prefix\":\"$PREFIX\"}" "$BASE/api/settings/storage" >/dev/null
ALBUM_ID=$(api -H 'Content-Type: application/json' -d '{"name":"S3 cleanup"}' "$BASE/api/albums" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
UPLOAD_JSON=$(api -X POST -F "files=@/opt/chronoframe-s3-cleanup/public/favicon-96x96.png;filename=live.png" "$BASE/api/albums/$ALBUM_ID/photos")
LIVE_KEY=$(printf '%s' "$UPLOAD_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["storageKey"])')
LIVE_OBJECT="$PREFIX/$LIVE_KEY"
ORPHAN_OBJECT="$PREFIX/albums/$ALBUM_ID/original/orphan.webp"
OUTSIDE_OBJECT="$PREFIX/unrelated/keep.bin"
printf 'old orphan payload' | docker exec -i "$MC_CONTAINER" mc pipe "e2e/$BUCKET/$ORPHAN_OBJECT" >/dev/null
printf 'outside payload' | docker exec -i "$MC_CONTAINER" mc pipe "e2e/$BUCKET/$OUTSIDE_OBJECT" >/dev/null

api -X POST "$BASE/api/s3-cleanups/scan" >/dev/null
for _ in $(seq 1 120); do
  SCAN_JSON=$(api "$BASE/api/s3-cleanups/latest")
  STATUS=$(printf '%s' "$SCAN_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')
  [[ "$STATUS" != "running" ]] && break
  sleep 0.25
done
printf '%s' "$SCAN_JSON" | python3 -c 'import json,sys; j=json.load(sys.stdin); assert j["status"] == "ready", j; assert j["scannedObjects"] == 2, j; assert j["total"] == 0, j; assert j["protectedObjects"] == 2, j'

DB_PATH="$(docker volume inspect -f '{{.Mountpoint}}' "$DATA_VOLUME")/chronoframe.db"
LOCATION_KEY="s3:http://$MINIO_CONTAINER:9000:us-east-1:$BUCKET:$PREFIX"
python3 - "$DB_PATH" "$LOCATION_KEY" "$PREFIX/albums/" "$ORPHAN_OBJECT" "$LIVE_OBJECT" "$LIVE_KEY" <<'PY'
import sqlite3, sys, time
db, location, prefix, orphan, live_object, live_key = sys.argv[1:]
now = int(time.time()) + 1
con = sqlite3.connect(db, timeout=30)
con.execute("INSERT INTO s3_cleanup_jobs(id,status,phase,scanned_objects,protected_objects,total,worker_count,location_key,managed_prefix,created_at,updated_at,bytes_found) VALUES('manual-delete','ready','ready',2,0,2,8,?,?,?,?,36)", (location, prefix, now, now))
con.execute("INSERT INTO s3_cleanup_items(id,job_id,object_key,logical_key,byte_size,last_modified,status) VALUES('orphan','manual-delete',?,?,18,1,'queued')", (orphan, orphan.removeprefix(prefix.rsplit('/albums/', 1)[0] + '/')))
con.execute("INSERT INTO s3_cleanup_items(id,job_id,object_key,logical_key,byte_size,last_modified,status) VALUES('live','manual-delete',?,?,18,1,'queued')", (live_object, live_key))
con.commit()
PY

api -X POST "$BASE/api/s3-cleanups/manual-delete/delete" >/dev/null
for _ in $(seq 1 120); do
  CLEANUP_JSON=$(api "$BASE/api/s3-cleanups/latest")
  STATUS=$(printf '%s' "$CLEANUP_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')
  [[ "$STATUS" != "running" ]] && break
  sleep 0.25
done
printf '%s' "$CLEANUP_JSON" | python3 -c 'import json,sys; j=json.load(sys.stdin); assert j["status"] == "completed", j; assert j["deleted"] == 1, j; assert j["skipped"] == 1, j; assert j["failed"] == 0, j'
docker exec "$MC_CONTAINER" mc stat "e2e/$BUCKET/$LIVE_OBJECT" >/dev/null
docker exec "$MC_CONTAINER" mc stat "e2e/$BUCKET/$OUTSIDE_OBJECT" >/dev/null
if docker exec "$MC_CONTAINER" mc stat "e2e/$BUCKET/$ORPHAN_OBJECT" >/dev/null 2>&1; then
  echo "orphan object still exists" >&2
  exit 1
fi

echo "S3 cleanup E2E passed: recent grace, managed-prefix scope, live reference guard, and orphan deletion"
