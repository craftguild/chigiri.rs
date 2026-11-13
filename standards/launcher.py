#!/usr/bin/env python3
"""Standards-owned fixed-reference acquisition and verified engine dispatch."""

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import signal
import subprocess
import sys
import tempfile

PROTOCOL = 'craftguild-execution-1'
ENTRY_PROTOCOL = 'craftguild-entry-1'
INSTRUCTION = 'Run make setup explicitly; ordinary operations never acquire an engine.'
OPERATIONS = {'setup', 'init', 'prime', 'build', 'prepare', 'install', 'installer',
              'package', 'test', 'check', 'acceptance', 'release', 'banner', 'assess',
              'verify', 'verify-frontend', 'verify-receipt', 'validate-sbom',
              'check-release-config', 'measure', 'measure-conditional', 'protocol'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def owned(root, name):
    relative = PurePosixPath(name)
    if (not name or relative.is_absolute() or '..' in relative.parts
            or str(relative) != name or '\\' in name or ':' in name):
        raise ValueError('Unsafe entry path: ' + name)
    path = root
    for part in relative.parts:
        path = path / part
        if path.is_symlink():
            raise ValueError('Symlink in entry/cache path: ' + name)
        if path.exists() and not (path.is_file() or path.is_dir()):
            raise ValueError('Nonregular entry/cache path: ' + name)
    return path


def hexadecimal(value, size):
    return isinstance(value, str) and len(value) == size and all(c in '0123456789abcdef' for c in value)


def host():
    system, machine = platform.system(), platform.machine()
    architecture = {'arm64': 'aarch64', 'aarch64': 'aarch64',
                    'amd64': 'x86_64', 'x86_64': 'x86_64'}.get(machine)
    suffix = {'Darwin': 'apple-darwin', 'Linux': 'unknown-linux-gnu'}.get(system)
    if not architecture or not suffix:
        raise ValueError('Unsupported engine host: ' + system + '/' + machine)
    return architecture + '-' + suffix


def reference(root, entry):
    path = owned(root, str(entry.relative_to(root) / 'engine.json'))
    data = path.read_bytes()
    pin = json.loads(data)
    if set(pin) != {'schema_version', 'release', 'entries', 'managed'} or pin['schema_version'] != 3:
        raise ValueError('Incompatible fixed reference; explicit migration required')
    release = pin['release']
    if set(release) != {'schema_version', 'identity', 'hosts'} or release['schema_version'] != 3:
        raise ValueError('Incompatible release metadata')
    identity = release['identity']
    if (set(identity) != {'version', 'revision', 'baseline', 'protocol', 'entry_protocol'}
            or identity['baseline'] != '0.1.0' or identity['protocol'] != PROTOCOL
            or identity['entry_protocol'] != ENTRY_PROTOCOL
            or not hexadecimal(identity['revision'], 40)):
        raise ValueError('Incompatible engine identity')
    version = identity['version']
    if not isinstance(version, str) or len(version.split('.')) != 3 or any(
            not p.isascii() or not p.isdecimal() or (len(p) > 1 and p[0] == '0')
            or int(p) > 2**64 - 1 for p in version.split('.')):
        raise ValueError('Expected stable engine version')
    entries = pin['entries']
    if set(entries) != {'launcher.py', 'make/common.mk'}:
        raise ValueError('Invalid entry ownership')
    for name, expected in entries.items():
        path = owned(root, str(entry.relative_to(root) / name))
        if not hexadecimal(expected, 64) or digest(path.read_bytes()) != expected:
            raise ValueError('Managed entry hash differs from fixed reference')
    if (set(pin['managed']) != {'.agents/skills/craftguild-standardize/SKILL.md', 'AGENTS.md'}
            or any(not hexadecimal(value, 64) for value in pin['managed'].values())):
        raise ValueError('Invalid managed baseline ownership')
    selected = host()
    if selected not in release['hosts']:
        raise ValueError('No published engine asset pinned for host ' + selected)
    roles = release['hosts'][selected]
    if set(roles) != {'initializer', 'engine'}:
        raise ValueError('Invalid release artifact roles')
    assets = roles['engine']
    if set(assets) != {'binary', 'sbom'}:
        raise ValueError('Invalid engine assets')
    base = f'https://github.com/craftguild/standards.rs/releases/download/v{version}/craftguild-standards-engine-v{version}-{selected}'
    for name, suffix in [('binary', ''), ('sbom', '.bom.json')]:
        asset = assets[name]
        if (set(asset) != {'url', 'sha256'} or asset['url'] != base + suffix
                or not hexadecimal(asset['sha256'], 64)):
            raise ValueError('Expected exact HTTPS release asset and SHA-256')
    return pin, digest(data), selected


def query(binary, operation, root):
    result = subprocess.run([str(binary), operation], cwd=root, capture_output=True,
                            check=True, timeout=10)
    if result.stderr:
        raise ValueError('Unexpected engine identity diagnostics')
    return result.stdout


def verify(root, directory, pin, reference_hash, selected):
    binary = owned(root, str(directory.relative_to(root) / 'craftguild-standards-engine'))
    sbom = owned(root, str(directory.relative_to(root) / 'engine.bom.json'))
    assets = pin['release']['hosts'][selected]['engine']
    for name, path in [('binary', binary), ('sbom', sbom)]:
        if not path.is_file() or digest(path.read_bytes()) != assets[name]['sha256']:
            raise ValueError('Cached/downloaded engine digest differs; ' + INSTRUCTION)
    expected = {'command': 'craftguild-standards-engine', 'engine': pin['release']['identity'], 'target': selected, 'clean': True}
    # Hash verification always precedes executing downloaded code.
    if json.loads(query(binary, 'engine-identity', root)) != expected:
        raise ValueError('Engine revision/version/compatibility differs from reference')
    if query(binary, 'sbom', root) != sbom.read_bytes():
        raise ValueError('Engine embedded SBOM differs from the pinned distribution')
    return binary, {'reference_sha256': reference_hash, 'identity': expected}


def acquire(root, cache, pin, reference_hash, selected):
    control = owned(root, '.craftguild')
    control.mkdir(exist_ok=True)
    lock = owned(root, '.craftguild/setup.lock')
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        if cache.exists():
            return
        cache.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='setup-', dir=cache.parent) as name:
            stage = Path(name)
            for asset, filename in [('binary', 'craftguild-standards-engine'), ('sbom', 'engine.bom.json')]:
                subprocess.run(['curl', '-fsSL', '--proto', '=https', '--proto-redir', '=https',
                                '--output', str(stage / filename),
                                pin['release']['hosts'][selected]['engine'][asset]['url']], check=True)
            (stage / 'craftguild-standards-engine').chmod(0o755)
            _, record = verify(root, stage, pin, reference_hash, selected)
            (stage / 'verified.json').write_text(json.dumps(record, sort_keys=True) + '\n')
            candidate = stage / 'ready'
            candidate.mkdir()
            for filename in ['craftguild-standards-engine', 'engine.bom.json', 'verified.json']:
                (stage / filename).rename(candidate / filename)
            # The exclusive setup lock serializes common-entry publication.
            if cache.exists():
                raise ValueError('Engine cache changed during setup')
            candidate.rename(cache)
    finally:
        identity = os.fstat(descriptor)
        os.close(descriptor)
        if lock.exists() and not lock.is_symlink() and lock.stat().st_ino == identity.st_ino:
            lock.unlink()


def engine(root, entry, operation):
    pin, reference_hash, selected = reference(root, entry)
    cache = owned(root, '.craftguild/engine/' + reference_hash + '/' + selected)
    if not cache.exists():
        if operation != 'setup':
            raise ValueError('Matching engine cache is missing. ' + INSTRUCTION)
        acquire(root, cache, pin, reference_hash, selected)
    binary, record = verify(root, cache, pin, reference_hash, selected)
    receipt = owned(root, str(cache.relative_to(root) / 'verified.json'))
    if json.loads(receipt.read_bytes()) != record:
        raise ValueError('Engine cache verification metadata differs')
    return binary


def interrupted(signum, frame):
    raise KeyboardInterrupt('Entry interrupted')


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in OPERATIONS:
        raise ValueError('Usage: launcher.py setup|init|prime|build|prepare|install|test|check|release|package')
    operation = sys.argv[1]
    if operation == 'setup' and len(sys.argv) != 2:
        raise ValueError('Usage: launcher.py setup')
    root = Path.cwd()
    script = Path(__file__).absolute()
    # Normalize only cwd's OS alias (for example macOS /var -> /private/var).
    # Retain every component below cwd so owned() still rejects local symlinks.
    anchor = next((parent for parent in script.parents if parent.resolve() == root), None)
    if anchor is None:
        raise ValueError('The common entry must be inside explicit cwd')
    entry = owned(root, str(script.relative_to(anchor).parent))
    binary = engine(root, entry, operation)
    if operation == 'setup':
        print(binary)
    else:
        forwarded = 'prime-context' if operation == 'prime' else operation
        os.execv(binary, [str(binary), forwarded, *sys.argv[2:]])


if __name__ == '__main__':
    for event in [signal.SIGTERM, signal.SIGINT]:
        signal.signal(event, interrupted)
    try:
        main()
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        print(f'Craftguild entry failed: {error}. {INSTRUCTION}', file=sys.stderr)
        sys.exit(1)
