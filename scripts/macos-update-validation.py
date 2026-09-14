"""Public payload validation. No downloaded executable runs before trust checks."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import subprocess
import zipfile

ASSET = 'open-desktop-macos-arm64.zip'
RELEASES = 'https://github.com/matheus-cintra/open-desktop/releases'


def run(*args, timeout=30):
    return subprocess.check_output([str(a) for a in args], stderr=subprocess.STDOUT,
                                   timeout=timeout, text=True).strip()


def download(url, target):
    run('/usr/bin/curl', '--proto', '=https', '--proto-redir', '=https',
        '--tlsv1.2', '-fsSL', '--retry', '2', '--connect-timeout', '15',
        '--max-time', '180', url, '-o', target, timeout=600)


def public(stage, version):
    if version == 'latest':
        metadata = stage / 'release.json'
        download('https://api.github.com/repos/matheus-cintra/open-desktop/releases/latest', metadata)
        version = json.loads(metadata.read_text())['tag_name']
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?', version):
        raise ValueError('Use latest or a concrete vX.Y.Z release tag')
    print('Release: ' + version, flush=True)
    for name in (ASSET, 'SHA256SUMS-macos'):
        download(f'{RELEASES}/download/{version}/{name}', stage / name)
    validate_zip(stage)
    run('/usr/bin/ditto', '-x', '-k', stage / ASSET, stage / 'payload')
    return stage / 'payload/Open Desktop.app', version[1:]


def validate_zip(stage):
    entries = [line.split() for line in (stage / 'SHA256SUMS-macos').read_text().splitlines()]
    matches = [row for row in entries if len(row) >= 2 and row[1].lstrip('*') == ASSET]
    if len(matches) != 1 or len(matches[0]) != 2 or not re.fullmatch('[a-fA-F0-9]{64}', matches[0][0]):
        raise ValueError('Missing, duplicate or invalid checksum')
    digest = hashlib.sha256()
    with (stage / ASSET).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    if digest.hexdigest() != matches[0][0].lower():
        raise ValueError('Checksum mismatch')
    with zipfile.ZipFile(stage / ASSET) as archive:
        seen = set()
        for item in archive.infolist():
            parts = item.filename.rstrip('/').split('/')
            mode = item.external_attr >> 16
            if (parts[0] not in ('Open Desktop.app', '__MACOSX') or
                    any(p in ('', '.', '..') for p in parts) or
                    '\\' in item.filename or any(ord(c) < 32 for c in item.filename) or
                    stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR) or
                    item.filename.casefold() in seen):
                raise ValueError('Unsafe ZIP entry: ' + item.filename)
            seen.add(item.filename.casefold())
        if not seen or archive.testzip() is not None:
            raise ValueError('Invalid ZIP')


def signature(app, stage, label):
    run('/usr/bin/codesign', '--verify', '--deep', '--strict', app)
    requirement = run('/usr/bin/codesign', '-d', '-r-', app)
    requirement = next((s.split('designated => ', 1)[1] for s in requirement.splitlines()
                        if s.startswith('designated => ')), '')
    if not requirement:
        raise ValueError('Missing designated requirement')
    prefix = stage / label
    run('/usr/bin/codesign', '-d', '--extract-certificates=' + str(prefix), app)
    cert = Path(str(prefix) + '0').read_bytes()  # Refuse ad hoc signatures.
    return requirement, hashlib.sha256(cert).hexdigest()


def validate(app, stage, version=None, installed=None):
    if app.is_symlink() or any(p.is_symlink() for p in app.rglob('*')):
        raise ValueError('Bundle symlinks are not supported')
    with (app / 'Contents/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    for key, expected in {'CFBundleIdentifier': 'dev.mcintra.opendesk',
                          'CFBundleExecutable': 'opendesk', 'CFBundlePackageType': 'APPL'}.items():
        if info.get(key) != expected:
            raise ValueError('Invalid bundle ' + key)
    binary = app / 'Contents/MacOS/opendesk'
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError('Missing executable')
    if run('/usr/bin/lipo', '-archs', binary) != 'arm64':
        raise ValueError('Expected ARM64 executable')
    current = signature(app, stage, 'candidate-cert')
    if installed and installed.exists():
        previous = signature(installed, stage, 'installed-cert')
        run('/usr/bin/codesign', '--verify', '--strict', '-R', '=' + previous[0], app)
        if current != previous:
            raise ValueError('Signing identity or designated requirement changed')
    build = dict(line.split('=', 1) for line in
                 (app / 'Contents/Resources/BUILD.txt').read_text().splitlines() if '=' in line)
    actual = run(binary, '--version').removeprefix('opendesk ')
    if (actual != build.get('version') or (version and actual != version) or
            info.get('CFBundleShortVersionString') != actual.split('-')[0] or
            info.get('CFBundleVersion') != actual.split('-')[0]):
        raise ValueError('Bundle version mismatch')
    return current
