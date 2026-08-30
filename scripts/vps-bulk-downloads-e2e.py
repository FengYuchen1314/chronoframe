#!/usr/bin/env python3
"""VPS-only bulk downloads regression, against a fresh isolated fixture container.

Start chronoframe-bulk-downloads-test on port 18321 with /app/data bind-mounted
from /opt/chronoframe-bulk-downloads-20260831/fixture. Never targets production.
"""
import hashlib
import http.cookiejar
import importlib.util
import io
import json
import subprocess
import time
import urllib.error
import urllib.request
import uuid
import zipfile
from pathlib import Path


def main():
    root = Path('/opt/chronoframe-bulk-downloads-20260831/fixture').resolve()
    container = 'chronoframe-bulk-downloads-test'
    info = json.loads(subprocess.check_output(['docker', 'inspect', container]))[0]
    assert root.parent == Path('/opt/chronoframe-bulk-downloads-20260831')
    assert any(Path(m['Source']).resolve() == root and m['Destination'] == '/app/data' for m in info['Mounts'])
    assert info['NetworkSettings']['Ports']['8080/tcp'][0]['HostPort'] == '18321'
    base = 'http://127.0.0.1:18321'
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))

    def request(path, method='GET', body=None, status=200, auth=True, csrf=True, raw=False, headers=None):
        headers = dict(headers or {})
        if auth and csrf:
            headers['X-Requested-With'] = 'ChronoFrame'
            headers['X-CSRF-Token'] = next((c.value for c in jar if c.name == 'cf_csrf'), '')
        if body is not None and not isinstance(body, bytes):
            body = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        req = urllib.request.Request(base + path, data=body, method=method, headers=headers)
        try:
            response = (opener if auth else urllib.request.build_opener()).open(req, timeout=60)
        except urllib.error.HTTPError as error:
            response = error
        payload = response.read()
        assert response.status == status, (path, response.status, payload[:1000])
        return payload if raw else json.loads(payload or b'null')

    def wait_for(check):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            result = check()
            if result:
                return result
            time.sleep(.3)
        raise AssertionError('background task timed out')

    def state():
        return request('/api/album-downloads')

    def ready(ids, count):
        def check():
            data = state()
            revisions = {s['albumId']: s['revision'] for s in data['settings']}
            jobs = [j for j in data['jobs'] if j['albumId'] in ids and j['revision'] == revisions[j['albumId']]]
            assert not any(j['status'] == 'failed' for j in jobs), jobs
            return jobs if len(jobs) == count and all(j['status'] == 'ready' for j in jobs) else None
        return wait_for(check)

    def apply(target, enabled=True, formats=None, **kwargs):
        return request('/api/album-downloads/settings/bulk', 'PUT', {
            'target': target, 'settings': {'enabled': enabled, 'formats': formats or ['png', 'webp'], 'maxImageBytes': 1000000}}, **kwargs)

    assert request('/api/albums', auth=False) == [], 'refusing nonempty gallery'
    assert not request('/api/auth/status', auth=False)['initialized'], 'requires fresh fixture'
    request('/api/auth/register', 'POST', {'username': 'bulk-test', 'password': 'Isolated-Bulk-Download-Test-2026!'}, status=201)
    albums = [request('/api/albums', 'POST', {'name': name}, status=201)['id'] for name in ['海岸与日落', '城市片段', '保留单独设置']]
    spec = importlib.util.spec_from_file_location('fixture', Path(__file__).with_name('make-fixture.py'))
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    sample = root.parent / 'bulk-sample.png'
    fixture.generate(sample, 640, 480, 64)
    source = sample.read_bytes()
    for album in albums:
        boundary = uuid.uuid4().hex
        body = (f'--{boundary}\r\nContent-Disposition: form-data; name="files"; filename="sample.png"\r\nContent-Type: image/png\r\n\r\n').encode() + source + f'\r\n--{boundary}--\r\n'.encode()
        request(f'/api/albums/{album}/photos', 'POST', body, headers={'Content-Type': f'multipart/form-data; boundary={boundary}'})
    originals = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (root / 'storage').rglob('*') if p.is_file()}
    assert originals
    target = {'scope': 'selected', 'albumIds': [albums[0], albums[1], albums[0]]}
    before = state()['settings']
    apply(target, auth=False, status=401)
    apply(target, csrf=False, status=403)
    apply({'scope': 'selected', 'albumIds': []}, status=400)
    apply({'scope': 'selected', 'albumIds': [albums[0], 'missing']}, status=400)
    apply({'scope': 'all', 'albumIds': [albums[0]]}, status=422, raw=True)
    apply(target, formats=['gif'], status=400)
    assert state()['settings'] == before
    assert apply(target)['updated'] == 2
    settings = state()['settings']
    assert next(s for s in settings if s['albumId'] == albums[2]) == next(s for s in before if s['albumId'] == albums[2])
    jobs = ready(albums[:2], 4)
    for job in jobs:
        payload = request(f'/api/albums/{job["albumId"]}/downloads/{job["format"]}', auth=False, raw=True)
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            assert archive.testzip() is None and len(archive.namelist()) == 1
            assert archive.namelist()[0].endswith('.' + job['format'])
            assert len(archive.read(archive.namelist()[0])) <= 1000000
    print('selected override, untouched albums, validation, authentication, local ZIP generation: PASS', flush=True)
    assert apply({'scope': 'all'}, formats=['jpg'])['updated'] == 3
    ready(albums, 3)
    wait_for(lambda: all(not (root / 'album-downloads' / (j['id'] + '.zip')).exists() for j in jobs))
    settings = state()['settings']
    assert all(s['enabled'] and s['formats'] == ['jpg'] and s['maxImageBytes'] == 1000000 for s in settings)
    future = request('/api/albums', 'POST', {'name': '后来新建的相册'}, status=201)['id']
    assert not next(s for s in state()['settings'] if s['albumId'] == future)['enabled']
    subprocess.run(['docker', 'restart', container], check=True, stdout=subprocess.DEVNULL)
    def alive():
        try:
            return request('/api/auth/status', auth=False)
        except (OSError, urllib.error.URLError):
            return None
    wait_for(alive)
    assert [s for s in state()['settings'] if s['albumId'] != future] == settings
    assert apply({'scope': 'all'}, enabled=False)['updated'] == 4
    assert request('/api/album-downloads/public', auth=False) == []
    wait_for(lambda: not list((root / 'album-downloads').glob('*.zip')))
    assert originals == {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (root / 'storage').rglob('*') if p.is_file()}
    print('all override, obsolete ZIP cleanup, future album defaults, restart persistence, disable without original deletion: PASS', flush=True)
    # Leave useful public fixtures for the manual desktop/mobile browser check.
    apply({'scope': 'selected', 'albumIds': albums[:2]})
    ready(albums[:2], 4)
    request(f'/api/albums/{albums[2]}/download-settings', 'PUT', {'enabled': True, 'formats': ['webp'], 'maxImageBytes': 1000000})
    ready([albums[2]], 1)
    print('UI fixture ready:', albums, flush=True)


if __name__ == '__main__':
    main()
