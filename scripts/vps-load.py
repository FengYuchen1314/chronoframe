#!/usr/bin/env python3
"""Dependency-free mixed read-load and response-integrity check for the VPS E2E stack."""

from __future__ import annotations

import argparse
import json
from concurrent.futures import ThreadPoolExecutor, as_completed
from statistics import quantiles
from time import perf_counter
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def read_json(url: str, timeout: float):
    with urlopen(url, timeout=timeout) as response:
        if response.status != 200:
            raise AssertionError(f"{url}: HTTP {response.status}")
        return json.load(response)


def image_signature(data: bytes) -> bool:
    return (
        data.startswith(b"\x89PNG\r\n\x1a\n")
        or (data.startswith(b"\xff\xd8\xff") and data.endswith(b"\xff\xd9"))
        or (len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP")
    )


def percentile(values: list[float], index: int) -> float:
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    return quantiles(values, n=100, method="inclusive")[index]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default="http://127.0.0.1:8188")
    parser.add_argument("--requests", type=int, default=600)
    parser.add_argument("--concurrency", type=int, default=40)
    parser.add_argument("--timeout", type=float, default=20)
    parser.add_argument("--album-id")
    parser.add_argument("--photo-id")
    parser.add_argument("--job-id")
    parser.add_argument("--admin-token")
    parser.add_argument("--max-failures", type=int, default=0)
    parser.add_argument("--max-p95-ms", type=float, default=1500)
    args = parser.parse_args()
    if args.requests < 1 or args.concurrency < 1:
        parser.error("requests and concurrency must be positive")

    albums = read_json(f"{args.base}/api/albums", args.timeout)
    if not albums:
        raise SystemExit("load check needs at least one album")
    album_id = args.album_id
    photos = []
    if album_id:
        photos = read_json(f"{args.base}/api/albums/{album_id}/photos", args.timeout)
    else:
        for album in albums:
            candidate = read_json(f"{args.base}/api/albums/{album['id']}/photos", args.timeout)
            if candidate:
                album_id, photos = album["id"], candidate
                break
    if album_id is None or (not photos and not args.photo_id):
        raise SystemExit("load check needs at least one photo")
    photo_id = args.photo_id or photos[0]["id"]

    endpoints: list[tuple[str, str]] = [
        ("albums", f"{args.base}/api/albums"),
        ("photos", f"{args.base}/api/albums/{album_id}/photos"),
        ("file", f"{args.base}/api/photos/{photo_id}/file"),
    ]
    if args.job_id:
        endpoints.append(("job", f"{args.base}/api/conversions/{args.job_id}?items=false"))
    work = [endpoints[index % len(endpoints)] for index in range(args.requests)]

    def fetch(entry: tuple[str, str]) -> float:
        kind, url = entry
        started = perf_counter()
        headers = (
            {"X-Admin-Token": args.admin_token}
            if kind == "job" and args.admin_token
            else {}
        )
        try:
            with urlopen(Request(url, headers=headers), timeout=args.timeout) as response:
                if response.status != 200:
                    raise AssertionError(f"{kind} {url}: HTTP {response.status}")
                data = response.read()
        except HTTPError as error:
            body = error.read(512).decode("utf-8", errors="replace")
            raise AssertionError(f"{kind} {url}: HTTP {error.code}: {body}") from error
        if kind == "file":
            if not image_signature(data):
                raise AssertionError(f"{url}: invalid image signature")
        else:
            payload = json.loads(data)
            if kind == "albums" and not isinstance(payload, list):
                raise AssertionError(f"{url}: albums response is not a list")
            if kind == "photos" and not isinstance(payload, list):
                raise AssertionError(f"{url}: photos response is not a list")
            if kind == "job":
                job = payload["job"]
                if job["completed"] != job["succeeded"] + job["failed"] + job["cancelled"]:
                    raise AssertionError(f"{url}: inconsistent conversion counters")
                if not (0 <= job["completed"] <= job["total"]):
                    raise AssertionError(f"{url}: conversion progress out of range")
        return (perf_counter() - started) * 1000

    started = perf_counter()
    latencies: list[float] = []
    failures: list[str] = []
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = [pool.submit(fetch, entry) for entry in work]
        for future in as_completed(futures):
            try:
                latencies.append(future.result())
            except Exception as error:  # reported below with a stable non-zero exit
                failures.append(str(error))
    elapsed = perf_counter() - started
    latencies.sort()
    p95 = percentile(latencies, 94)
    result = {
        "requests": len(work),
        "successful": len(latencies),
        "concurrency": args.concurrency,
        "failures": len(failures),
        "elapsed_seconds": round(elapsed, 3),
        "requests_per_second": round(len(work) / elapsed, 2),
        "p50_ms": round(percentile(latencies, 49), 2),
        "p95_ms": round(p95, 2),
        "p99_ms": round(percentile(latencies, 98), 2),
        "max_ms": round(max(latencies), 2) if latencies else None,
        "sample_failure": failures[0] if failures else None,
        "limits": {"max_failures": args.max_failures, "max_p95_ms": args.max_p95_ms},
    }
    print(json.dumps(result, ensure_ascii=False))
    if len(failures) > args.max_failures or not latencies or p95 > args.max_p95_ms:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
