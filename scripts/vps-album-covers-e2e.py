#!/usr/bin/env python3
"""Run only on the VPS against the dedicated copied cover fixture, not production."""
import hashlib
import http.cookiejar
import json
import socket
import sqlite3
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path('/opt/chronoframe-album-covers-20260831/fixture')
BASE = 'http://127.0.0.1:18323'
CONTAINER = 'chronoframe-cover-test'


def main():
    container = json.loads(subprocess.check_output(['docker', 'inspect', CONTAINER]))[0]
    assert any(m['Source'] == str(ROOT) and m['Destination'] == '/app/data' for m in container['Mounts'])
    assert container['NetworkSettings']['Ports']['8080/tcp'][0]['HostPort'] == '18323'
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))

    def request(path, method='GET', body=None, expected=200, headers=None, anonymous=False):
        headers = dict(headers or {})
        if method != 'GET':
            headers.setdefault('X-Requested-With', 'ChronoFrame')
            headers.setdefault('X-CSRF-Token', next((c.value for c in jar if c.name == 'cf_csrf'), ''))
        if body is not None and not isinstance(body, bytes):
            body = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        client = urllib.request if anonymous else opener
        try:
            response = client.urlopen(urllib.request.Request(BASE + path, data=body, method=method, headers=headers), timeout=30) if anonymous else client.open(urllib.request.Request(BASE + path, data=body, method=method, headers=headers), timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        payload = response.read()
        assert response.status == expected, (path, response.status, payload[:500])
        return payload, response.headers

    def api(path, method='GET', body=None, **options):
        return json.loads(request(path, method, body, **options)[0])

    def upload(album_id, data, name='cover.png', extra=b'', expected=200, **options):
        body = b'--cover-boundary\r\nContent-Disposition: form-data; name="file"; filename="' + name.encode() + b'"\r\nContent-Type: application/octet-stream\r\n\r\n' + data + b'\r\n' + extra + b'--cover-boundary--\r\n'
        return request(f'/api/albums/{album_id}/cover', 'POST', body, expected=expected,
                       headers={'Content-Type': 'multipart/form-data; boundary=cover-boundary'}, **options)

    def cover_rows():
        with sqlite3.connect(f'file:{ROOT}/chronoframe.db?mode=ro', uri=True) as db:
            return db.execute('SELECT album_id,photo_id,length(image),version FROM album_covers ORDER BY album_id').fetchall()

    original = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT / 'storage').rglob('*') if p.is_file()}
    albums = api('/api/albums')
    assert len(albums) == 2 and original
    primary = next(a for a in albums if a['photoCount'] > 1)
    other = next(a for a in albums if a['id'] != primary['id'])
    photos = api(f'/api/albums/{primary["id"]}')['photos']
    foreign = api(f'/api/albums/{other["id"]}')['photos'][0]['id']
    png = request(f'/api/photos/{photos[0]["id"]}/thumbnail')[0]
    assert png.startswith(b'\x89PNG')
    route = f'/api/albums/{primary["id"]}/cover'
    request(route, 'PUT', {'photoId': photos[-1]['id']}, expected=401, anonymous=True)
    upload(primary['id'], png, expected=401, anonymous=True)
    api('/api/auth/login', 'POST', {'username': 'ant-test', 'password': 'Isolated-Ant-Download-Test-2026!'})
    request(route, 'DELETE', expected=403, headers={'X-CSRF-Token': 'invalid'})
    before_downloads = api('/api/album-downloads/public')
    selected = api(route, 'PUT', {'photoId': photos[-1]['id']})
    assert selected['coverSource'] == 'photo'
    request(selected['coverUrl'])
    request(route, 'PUT', {'photoId': foreign}, expected=400)
    request(route, 'PUT', {'photoId': 'missing'}, expected=400)
    assert api(f'/api/albums/{primary["id"]}')['coverPhotoId'] == photos[-1]['id']
    assert api(route, 'DELETE')['coverSource'] == 'auto'
    request(route, 'DELETE')
    print('Authentication, CSRF, same-album selection, invalid IDs, reset: PASS', flush=True)

    empty = api('/api/albums', 'POST', {'name': '独立封面测试', 'description': '仅用于封面测试，不包含照片'}, expected=201)
    created = json.loads(upload(empty['id'], png)[0])
    url = created['coverUrl']
    data, headers = request(url)
    assert data[:4] == b'RIFF' and data[8:12] == b'WEBP' and len(data) <= 200000
    assert headers['Content-Type'] == 'image/webp' and 'immutable' in headers['Cache-Control']
    assert api(f'/api/albums/{empty["id"]}')['photoCount'] == 0
    old = cover_rows()
    upload(empty['id'], b'<svg></svg>', expected=400)
    upload(empty['id'], png, name='wrong.jpg', expected=400)
    upload(empty['id'], png, extra=b'--cover-boundary\r\nContent-Disposition: form-data; name="extra"\r\n\r\nwrong\r\n', expected=400)
    assert cover_rows() == old
    # Abort an authenticated multipart request before its body completes.
    cookie = '; '.join(f'{c.name}={c.value}' for c in jar)
    csrf = next(c.value for c in jar if c.name == 'cf_csrf')
    with socket.create_connection(('127.0.0.1', 18323), timeout=5) as sock:
        head = f'POST /api/albums/{empty["id"]}/cover HTTP/1.1\r\nHost: 127.0.0.1:18323\r\nCookie: {cookie}\r\nX-CSRF-Token: {csrf}\r\nX-Requested-With: ChronoFrame\r\nContent-Type: multipart/form-data; boundary=broken\r\nContent-Length: 1000000\r\n\r\n'
        sock.sendall(head.encode() + b'--broken\r\nContent-Disposition: form-data; name="file"; filename="cover.png"\r\n\r\npartial')
    time.sleep(0.3)
    assert cover_rows() == old and request(url)[0] == data
    print('Empty-album upload, bounded WebP, invalid/multiple files and interrupted upload preserve old cover: PASS', flush=True)

    subprocess.check_call(['docker', 'restart', CONTAINER], stdout=subprocess.DEVNULL)
    for attempt in range(30):
        try:
            assert request(url)[0] == data
            break
        except (OSError, AssertionError):
            if attempt == 29:
                raise
            time.sleep(1)
    newer = json.loads(upload(empty['id'], png)[0])
    assert newer['coverUrl'] != url
    request(url, expected=404)
    api(f'/api/albums/{empty["id"]}', 'DELETE')
    request(newer['coverUrl'], expected=404)
    assert all(r[0] != empty['id'] for r in cover_rows())
    assert api('/api/album-downloads/public') == before_downloads
    assert original == {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT / 'storage').rglob('*') if p.is_file()}
    print('Restart persistence, versioned replacement, album cleanup, source photos and download ZIPs unchanged: PASS', flush=True)


if __name__ == '__main__':
    main()
