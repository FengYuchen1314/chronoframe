#!/usr/bin/env python3
"""Offline fixture for a disposable viewer test instance, never a production database."""
import argparse
import os
from pathlib import Path
import runpy
import sqlite3
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('phase', choices=['seed', 'expand'])
parser.add_argument('--data', type=Path, required=True)
parser.add_argument('--count', type=int, default=360)
args = parser.parse_args()
root = args.data.resolve()
if root.name != 'fixture-data' or root.parent.name != 'chronoframe-viewer-performance':
    raise SystemExit('Refusing: use the isolated chronoframe-viewer-performance/fixture-data directory')
db = sqlite3.connect(f'file:{root / "chronoframe.db"}?mode=rw', uri=True)
album = '11111111-1111-4111-8111-111111111111'
ids = [str(uuid.UUID(int=index + 1, version=4)) for index in range(args.count)]
dimensions = [(1280, 720), (600, 1000), (900, 900)]
stamp = int(time.time())

if args.phase == 'seed':
    if db.execute('select count(*) from albums').fetchone()[0] or db.execute('select count(*) from administrators').fetchone()[0]:
        raise SystemExit('Refusing to seed a non-empty installation')
    # Disable public registration/login on this throwaway instance.
    db.execute("insert into administrators(id,username,password_hash,created_at) values(1,'disabled-performance-fixture','disabled',?)", (stamp,))
    db.execute('insert into albums(id,name,description,created_at) values(?,?,?,?)', (album, 'PERFORMANCE FIXTURE', 'Synthetic images; no user data.', stamp))
    generate = runpy.run_path(str(Path(__file__).with_name('make-fixture.py')))['generate']
    for index, (width, height) in enumerate(dimensions):
        key = f'albums/{album}/{ids[index]}.png'
        path = root / 'storage' / key
        path.parent.mkdir(parents=True, exist_ok=True)
        generate(path, width, height, index)
        db.execute('insert into photos(id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at) values(?,?,?,?,?,?,?,?,?,?)',
                   (ids[index], album, f'Fixture {index + 1:04d}.png', key, 'png', 'image/png', path.stat().st_size, width, height, stamp - index))
else:
    admin = db.execute('select username from administrators').fetchone()
    name = db.execute('select name from albums where id=?', (album,)).fetchone()
    if admin != ('disabled-performance-fixture',) or name != ('PERFORMANCE FIXTURE',):
        raise SystemExit('Refusing to modify a non-fixture database')
    for index in range(3, args.count):
        source_index = index % 3
        source_key = f'albums/{album}/{ids[source_index]}.png'
        key = f'albums/{album}/{ids[index]}.png'
        path = root / 'storage' / key
        os.link(root / 'storage' / source_key, path)
        for suffix in ('grid.png', 'preview.webp', 'high.webp'):
            os.link(root / 'thumbnails' / f'{ids[source_index]}.{suffix}', root / 'thumbnails' / f'{ids[index]}.{suffix}')
        width, height = dimensions[source_index]
        db.execute('insert into photos(id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at) values(?,?,?,?,?,?,?,?,?,?)',
                   (ids[index], album, f'Fixture {index + 1:04d}.png', key, 'png', 'image/png', path.stat().st_size, width, height, stamp - index))
db.commit()
print({'phase': args.phase, 'photos': db.execute('select count(*) from photos').fetchone()[0], 'album': album})
