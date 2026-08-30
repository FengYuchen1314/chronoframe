#!/usr/bin/env python3
"""VPS-only test against the dedicated copied local fixture, never production."""
import hashlib
import http.cookiejar
import io
import json
import subprocess
import urllib.error
import urllib.request
import zipfile
from pathlib import Path


def main():
    root = Path('/opt/chronoframe-mobile-downloads-20260831/fixture')
    info = json.loads(subprocess.check_output(['docker', 'inspect', 'chronoframe-mobile-downloads-test']))[0]
    assert any(m['Source'] == str(root) and m['Destination'] == '/app/data' for m in info['Mounts'])
    assert info['NetworkSettings']['Ports']['8080/tcp'][0]['HostPort'] == '18322'
    base = 'http://127.0.0.1:18322'
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))

    def request(path, method='GET', body=None, status=200, headers=None):
        headers = dict(headers or {})
        if body is not None:
            body = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
            headers['X-Requested-With'] = 'ChronoFrame'
            headers['X-CSRF-Token'] = next((cookie.value for cookie in jar if cookie.name == 'cf_csrf'), '')
        try:
            response = opener.open(urllib.request.Request(base + path, method=method, data=body, headers=headers), timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        payload = response.read()
        assert response.status == status, (path, response.status, payload[:500])
        return payload, response.headers

    def public():
        return json.loads(request('/api/album-downloads/public')[0])

    original_files = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (root / 'storage').rglob('*') if p.is_file()}
    assert original_files
    downloads = public()
    assert len(downloads) == 2 and sorted(len(a['formats']) for a in downloads) == [1, 4], 'unexpected test fixture'
    seen = set()
    for album in downloads:
        for entry in album['formats']:
            assert entry['status'] == 'ready' and entry['photosUrl']
            data, headers = request(entry['url'])
            assert headers['Content-Type'] == 'application/zip'
            part, response = request(entry['url'], status=206, headers={'Range': 'bytes=2-99'})
            assert part == data[2:100]
            manifest = json.loads(request(entry['photosUrl'])[0])
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                assert archive.testzip() is None
                assert len(manifest['photos']) == len(archive.namelist())
                for photo in manifest['photos']:
                    image, response = request(photo['url'])
                    assert image == archive.read(photo['name'])
                    assert len(image) == photo['byteSize']
                    assert photo['name'].endswith('.' + entry['format'])
                    assert response['Content-Type'] == {'png': 'image/png', 'webp': 'image/webp', 'jpg': 'image/jpeg', 'jpeg': 'image/jpeg'}[entry['format']]
                    assert response['Cache-Control'] == 'private, no-store'
                    assert 'attachment;' in response['Content-Disposition']
                    chunk, response = request(photo['url'], status=206, headers={'Range': 'bytes=2-99'})
                    assert chunk == image[2:100]
                    assert response['Content-Range'] == f'bytes 2-99/{len(image)}'
                    full, _ = request(photo['url'], headers={'Range': 'bytes=2-99', 'If-Range': '"obsolete"'})
                    assert full == image
                    request(photo['url'], status=416, headers={'Range': 'bytes=9999999999-'})
                    request(photo['url'].split('?')[0] + '?version=old', status=404)
            request(entry['photosUrl'].replace('/photos?', '/photos/999999?'), status=404)
            request(entry['photosUrl'].split('?')[0] + '?version=old', status=404)
            seen.add(entry['format'])
    assert seen == {'png', 'jpg', 'jpeg', 'webp'}
    print('PNG/JPG/JPEG/WebP entries exactly match ZIP bytes, filenames, limits and MIME; desktop ZIP + relative Range/If-Range/stale versions: PASS', flush=True)
    # Revoke only the throwaway single-format album; leave the primary for UI checks.
    revoked = next(a for a in downloads if len(a['formats']) == 1)
    old_list = revoked['formats'][0]['photosUrl']
    old_photo = json.loads(request(old_list)[0])['photos'][0]['url']
    request('/api/auth/login', 'POST', {'username': 'ant-test', 'password': 'Isolated-Ant-Download-Test-2026!'})
    request(f'/api/albums/{revoked["albumId"]}/download-settings', 'PUT', {'enabled': False, 'formats': ['webp'], 'maxImageBytes': 4000000})
    request(old_list, status=404)
    request(old_photo, status=404)
    assert revoked['albumId'] not in [a['albumId'] for a in public()]
    assert original_files == {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (root / 'storage').rglob('*') if p.is_file()}
    print('Cached manifest and photo URLs are immediately revoked; original storage bytes unchanged: PASS', flush=True)


if __name__ == '__main__':
    main()
