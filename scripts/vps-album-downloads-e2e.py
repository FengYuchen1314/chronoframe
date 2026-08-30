#!/usr/bin/env python3
"""Run ONLY on a VPS against a fresh, isolated downloads-test container.

The test deliberately restarts its own container and modifies its fixture database.
It refuses production URLs, container names, or nonempty galleries.
"""
import argparse
import hashlib
import http.cookiejar
import importlib.util
import io
import json
import sqlite3
import subprocess
import time
import urllib.error
import urllib.request
import uuid
import zipfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--backend', choices=['local', 's3', 'webdav'], default='local')
    args = parser.parse_args()
    backend = args.backend
    port = {'local': 18311, 's3': 18312, 'webdav': 18313}[backend]
    base = f'http://127.0.0.1:{port}'
    container = f'chronoframe-ant-downloads-test-{backend}'
    root = Path(f'/opt/chronoframe-ant-downloads-20260831/fixture-{backend}').resolve()
    assert root.parent == Path('/opt/chronoframe-ant-downloads-20260831')
    mount = json.loads(subprocess.check_output(['docker', 'inspect', container]))[0]
    assert any(Path(m['Source']).resolve() == root and m['Destination'] == '/app/data' for m in mount['Mounts'])
    assert mount['NetworkSettings']['Ports']['8080/tcp'][0]['HostPort'] == str(port)
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))

    def request(path, method='GET', body=None, auth=True, status=200, headers=None, raw=False):
        h = dict(headers or {})
        if auth:
            h['X-Requested-With'] = 'ChronoFrame'
            csrf = next((c.value for c in jar if c.name == 'cf_csrf'), '')
            if csrf:
                h['X-CSRF-Token'] = csrf
        if body is not None and not isinstance(body, bytes):
            body = json.dumps(body).encode()
            h['Content-Type'] = 'application/json'
        req = urllib.request.Request(base + path, data=body, method=method, headers=h)
        try:
            res = (opener if auth else urllib.request.build_opener()).open(req, timeout=90)
        except urllib.error.HTTPError as error:
            res = error
        payload = res.read()
        assert res.status == status, (path, res.status, payload[:1000])
        return (payload, res.headers) if raw else json.loads(payload or b'null')

    def wait_for(check, timeout=120):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = check()
            if value:
                return value
            time.sleep(.3)
        raise AssertionError('timed out waiting for background work')

    def state():
        return request('/api/album-downloads')

    def config(album, enabled=True, formats=None, cap=20000):
        return request(f'/api/albums/{album}/download-settings', 'PUT', {
            'enabled': enabled, 'formats': formats or ['png', 'jpg', 'jpeg', 'webp'], 'maxImageBytes': cap})

    def current_jobs(album):
        data = state()
        rev = next(s['revision'] for s in data['settings'] if s['albumId'] == album)
        return [j for j in data['jobs'] if j['albumId'] == album and j['revision'] == rev]

    def ready(album, count=4):
        def check():
            jobs = current_jobs(album)
            assert not any(j['status'] == 'failed' for j in jobs), jobs
            return jobs if len(jobs) == count and all(j['status'] == 'ready' for j in jobs) else None
        return wait_for(check)

    existing = request('/api/albums', auth=False)
    assert all(a['name'] == '下载测试 / 相册' and a['description'] == 'Ant Design 集成测试' and a['photoCount'] == 0 for a in existing), 'refusing to modify a nonempty gallery'
    request('/api/album-downloads', auth=False, status=401)
    initialized = request('/api/auth/status', auth=False)['initialized']
    request('/api/auth/login' if initialized else '/api/auth/register', 'POST', {'username': 'ant-test', 'password': 'Isolated-Ant-Download-Test-2026!'}, status=200 if initialized else 201)
    for album in existing:
        request(f'/api/albums/{album["id"]}', 'DELETE')
    if backend == 's3':
        request('/api/settings/storage', 'PUT', {'backend': 's3', 's3Endpoint': 'http://chronoframe-ant-downloads-test-minio:9000', 's3Region': 'us-east-1', 's3Bucket': 'downloads-test', 's3AccessKey': 'ant-e2e-access', 's3SecretKey': 'ant-e2e-secret-2026', 's3Prefix': 'photos'})
    if backend == 'webdav':
        request('/api/settings/storage', 'PUT', {'backend': 'webdav', 'webdavUrl': 'http://chronoframe-ant-downloads-test-dav-server', 'webdavUsername': 'ant-e2e', 'webdavPassword': 'ant-e2e-dav-2026', 'webdavPrefix': 'photos'})
    album = request('/api/albums', 'POST', {'name': '下载测试 / 相册', 'description': 'Ant Design 集成测试'}, status=201)['id']
    second = request('/api/albums', 'POST', {'name': '单格式下载示例'}, status=201)['id']
    assert request('/api/album-downloads/public', auth=False) == []
    request(f'/api/albums/{album}/downloads/png', auth=False, status=404)
    request(f'/api/albums/{album}/download-settings', 'PUT', {'enabled': True, 'formats': ['gif'], 'maxImageBytes': 20000}, status=400)
    request(f'/api/albums/{album}/download-settings', 'PUT', {'enabled': True, 'formats': ['png'], 'maxImageBytes': 1}, status=400)
    request(f'/api/albums/{album}/download-settings', 'PUT', {'enabled': True, 'formats': ['png'], 'maxImageBytes': 20000}, auth=False, status=401)
    # A session cookie without its CSRF token must not authorize configuration changes.
    no_csrf = urllib.request.Request(base + f'/api/albums/{album}/downloads/rebuild', data=b'', method='POST')
    try:
        opener.open(no_csrf)
        raise AssertionError('CSRF guard failed')
    except urllib.error.HTTPError as error:
        assert error.code == 403

    spec = importlib.util.spec_from_file_location('fixture', Path(__file__).with_name('make-fixture.py'))
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    sample = root.parent / f'sample-{backend}.png'
    fixture.generate(sample, 640, 480, 1024)
    source = sample.read_bytes()

    def upload(target, name):
        boundary = uuid.uuid4().hex
        data = (f'--{boundary}\r\nContent-Disposition: form-data; name="files"; filename="{name}"\r\nContent-Type: image/png\r\n\r\n').encode() + source + f'\r\n--{boundary}--\r\n'.encode()
        return request(f'/api/albums/{target}/photos', 'POST', data, headers={'Content-Type': f'multipart/form-data; boundary={boundary}'})[0]

    photos = [upload(album, name) for name in ['same.png', 'same.png', '../folder\\photo.png']]
    upload(second, 'only.png')
    config(album)
    jobs = ready(album)
    download_root = root / 'album-downloads'
    assert len(list(download_root.glob('*.zip'))) == 4
    for job in jobs:
        assert job['completed'] == job['total'] == 3
        fmt = job['format']
        payload, headers = request(f'/api/albums/{album}/downloads/{fmt}', auth=False, raw=True)
        assert payload == (download_root / (job['id'] + '.zip')).read_bytes()
        assert headers['Content-Type'] == 'application/zip'
        assert 'attachment;' in headers['Content-Disposition']
        etag = headers['ETag']
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            assert archive.testzip() is None
            names = archive.namelist()
            assert len(names) == len(set(names)) == 3
            for name in names:
                assert '/' not in name and '\\' not in name and name.endswith('.' + fmt)
                data = archive.read(name)
                assert len(data) <= 20000
                assert data.startswith(b'\x89PNG') if fmt == 'png' else (data[8:12] == b'WEBP' if fmt == 'webp' else data.startswith(b'\xff\xd8'))
        part, response = request(f'/api/albums/{album}/downloads/{fmt}', auth=False, status=206, headers={'Range': 'bytes=2-99'}, raw=True)
        assert part == payload[2:100] and response['Content-Range'].startswith('bytes 2-99/')
        unchanged, _ = request(f'/api/albums/{album}/downloads/{fmt}', auth=False, status=206, headers={'Range': 'bytes=2-99', 'If-Range': etag}, raw=True)
        assert unchanged == payload[2:100]
        replaced, _ = request(f'/api/albums/{album}/downloads/{fmt}', auth=False, headers={'Range': 'bytes=2-99', 'If-Range': '"old-version"'}, raw=True)
        assert replaced == payload
        request(f'/api/albums/{album}/downloads/{fmt}?version=old-version', auth=False, status=404)
        request(f'/api/albums/{album}/downloads/{fmt}', auth=False, status=416, headers={'Range': 'bytes=999999999999-'})
    print(f'{backend}: formats, per-image limits, names, local files, public auth and range OK', flush=True)

    job = jobs[0]
    request(f'/api/album-downloads/{job["id"]}', 'DELETE')
    request(f'/api/albums/{album}/downloads/{job["format"]}', auth=False, status=404)
    wait_for(lambda: not (download_root / (job['id'] + '.zip')).exists())
    time.sleep(3)
    assert next(j for j in current_jobs(album) if j['id'] == job['id'])['status'] == 'deleted'
    request(f'/api/albums/{album}/downloads/rebuild', 'POST')
    jobs = ready(album)
    old_ids = [j['id'] for j in jobs]
    upload(album, 'new.png')
    request(f'/api/albums/{album}/downloads/png', auth=False, status=404)
    jobs = ready(album)
    assert all(j['total'] == 4 for j in jobs)
    wait_for(lambda: all(not (download_root / (id + '.zip')).exists() for id in old_ids))
    request(f'/api/albums/{album}', 'PATCH', {'name': '相册已改名', 'description': '完整下载测试'})
    ready(album)
    request(f'/api/photos/{photos[-1]["id"]}', 'DELETE')
    assert all(j['total'] == 3 for j in ready(album))
    # Disabling immediately withdraws old URLs, while cleanup can finish asynchronously.
    config(album, enabled=False)
    request(f'/api/albums/{album}/downloads/png', auth=False, status=404)
    wait_for(lambda: not list(download_root.glob('*.zip')))
    assert len(request(f'/api/albums/{album}/photos')) == 3
    print(f'{backend}: deletion tombstone, rebuild, content/rename invalidation and disable cleanup OK', flush=True)

    if backend == 'local':
        small_source = source
        fixture.generate(sample, 2048, 1536, 4096)
        source = sample.read_bytes()
        heavy = upload(album, 'cancellation-fixture.png')
        source = small_source
        config(album)
        running = wait_for(lambda: next((j for j in current_jobs(album) if j['status'] == 'running' and j['completed'] < j['total']), None))
        request(f'/api/album-downloads/{running["id"]}/cancel', 'POST')
        wait_for(lambda: next(j for j in current_jobs(album) if j['id'] == running['id'])['status'] == 'cancelled')
        request(f'/api/albums/{album}/downloads/{running["format"]}', auth=False, status=404)
        config(album, enabled=False)
        wait_for(lambda: not list(download_root.glob('*.work')) and not list(download_root.glob('*.zip')))
        request(f'/api/photos/{heavy["id"]}', 'DELETE')
        print('local: cancellation of an actively encoding task and temporary-file cleanup OK', flush=True)
        original = (root / 'storage' / photos[0]['storageKey']).resolve()
        assert original.is_relative_to(root / 'storage')
        backup = original.with_suffix('.test-backup')
        original.rename(backup)
        try:
            config(album, formats=['png'])
            wait_for(lambda: any(j['status'] == 'failed' for j in current_jobs(album)))
            wait_for(lambda: not list(download_root.glob('*.work')) and not list(download_root.glob('*.zip')))
            request(f'/api/albums/{album}/downloads/png', auth=False, status=404)
        finally:
            backup.rename(original)
        print('local: failed source read publishes no partial archive and cleans temporary files OK', flush=True)

    # Inject an interrupted task into THIS isolated, stopped database; startup must recover it.
    config(album, formats=['png'])
    job = ready(album, 1)[0]
    subprocess.run(['docker', 'stop', container], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(root / 'chronoframe.db') as db:
        db.execute("UPDATE album_download_jobs SET status='running',completed=1 WHERE id=?", (job['id'],))
    (download_root / (job['id'] + '.part')).write_bytes(b'incomplete')
    subprocess.run(['docker', 'start', container], check=True, stdout=subprocess.DEVNULL)
    def restarted():
        try:
            return request('/api/auth/status')['initialized']
        except (urllib.error.URLError, ConnectionError):
            return False
    wait_for(restarted)
    recovered = ready(album, 1)[0]
    assert recovered['id'] == job['id']
    assert not (download_root / (job['id'] + '.part')).exists()
    # Hold a task queued until its cancel/delete request: cancellation must survive restart.
    subprocess.run(['docker', 'stop', container], check=True, stdout=subprocess.DEVNULL)
    cancelled = str(uuid.uuid4())
    with sqlite3.connect(root / 'chronoframe.db') as db:
        db.execute('UPDATE album_download_settings SET revision=revision+1,updated_at=? WHERE album_id=?', (int(time.time()) + 60, album))
        rev = db.execute('SELECT revision FROM album_download_settings WHERE album_id=?', (album,)).fetchone()[0]
        db.execute("INSERT INTO album_download_jobs(id,album_id,format,revision,status,created_at,updated_at) VALUES(?,?, 'png',?,'cancelled',?,?)", (cancelled, album, rev, int(time.time()), int(time.time())))
    (download_root / (cancelled + '.part')).write_bytes(b'cancelled garbage')
    subprocess.run(['docker', 'start', container], check=True, stdout=subprocess.DEVNULL)
    wait_for(restarted)
    wait_for(lambda: not (download_root / (cancelled + '.part')).exists())
    assert current_jobs(album)[0]['status'] == 'cancelled'
    request(f'/api/albums/{album}/downloads/png', auth=False, status=404)
    print(f'{backend}: restart recovery and persistent cancellation OK', flush=True)
    # Leave meaningful public fixtures for browser QA.
    config(album)
    ready(album)
    config(second, formats=['webp'], cap=5000000)
    ready(second, 1)
    assert not list(download_root.glob('*.part'))
    wait_for(lambda: not list(download_root.glob('*.work')))
    if backend == 's3':
        listing = subprocess.check_output(['docker', 'exec', 'chronoframe-ant-downloads-test-mc', 'mc', 'ls', '--recursive', '--json', 'e2e/downloads-test']).decode()
        objects = [json.loads(line) for line in listing.splitlines() if line.strip()]
        keys = [obj['key'] for obj in objects if obj.get('type') == 'file']
        assert not any(key.endswith(('.zip', '.part')) for key in keys), keys
        assert len([key for key in keys if '/original/' in key]) == 4, keys
        print('s3: original objects retained, zero remote ZIP/part objects OK', flush=True)
    if backend == 'webdav':
        files = subprocess.check_output(['docker', 'exec', 'chronoframe-ant-downloads-test-dav-server', 'find', '/var/lib/dav', '-type', 'f']).decode().splitlines()
        assert not any(path.endswith(('.zip', '.part')) for path in files), files
        assert len([path for path in files if '/original/' in path]) == 4, files
        print('webdav: original objects retained, zero remote ZIP/part files OK', flush=True)
    print(json.dumps({'backend': backend, 'album': album, 'second': second, 'zipCount': len(list(download_root.glob('*.zip'))), 'sha256Source': hashlib.sha256(source).hexdigest()}), flush=True)


if __name__ == '__main__':
    main()
