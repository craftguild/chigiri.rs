#!/usr/bin/env python3
"""Package a verified common build through the retained secure product publisher."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

from sdk_preflight import ROOT, TARGETS


def shared(*args):
    subprocess.run(['sh', 'tools/standards.sh', *args], cwd=ROOT, check=True)


def verified_bytes(path, expected):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise ValueError('Common distribution output must be a regular file')
        data = source.read()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('Common output changed during distribution packaging')
    return data


def package(target):
    if target not in TARGETS:
        raise ValueError('Select one supported distribution target')
    slot = ROOT / 'target/dist' / target
    for path in [ROOT / 'target', slot.parent, slot]:
        if path.is_symlink():
            raise ValueError('distribution path is a symlink: ' + str(path))
    shared('build', '--artifact', 'chigiri-cli', '--target', target,
           '--profile', 'release', '--variant', 'distribution')
    shared('verify-receipt', '--artifact', 'chigiri-cli')
    receipt = json.loads((ROOT / '.craftguild/chigiri/receipt.json').read_text())
    context = receipt['context']
    if (context['target'], context['profile'], context['variant']) != (target, 'release', 'distribution'):
        raise ValueError('Common receipt does not describe the requested distribution')
    name = context['file']
    data = verified_bytes(Path(context['output']), receipt['outputs'][name])
    host = context['host']
    if not host or host not in TARGETS:
        raise ValueError('Distribution publisher requires a recorded qualified build host')
    helper_dir = ROOT / '.craftguild/generators/host-publish'
    env = dict(os.environ, CARGO_TARGET_DIR=str(helper_dir))
    for key in ['CRAFTGUILD_PREPARED', 'CRAFTGUILD_COMMAND']:
        env.pop(key, None)
    subprocess.run(['cargo', '+1.95.0', 'build', '--locked', '--target', host,
                    '-p', 'chigiri_host', '--bin', 'chigiri-host-publish'],
                   cwd=ROOT, env=env, check=True)
    helper = helper_dir / host / 'debug/chigiri-host-publish'
    if helper.is_symlink() or not helper.is_file() or not os.access(helper, os.X_OK):
        raise ValueError('Missing regular executable publication helper')
    for path in [ROOT / 'target', slot.parent, slot]:
        if path.is_symlink():
            raise ValueError('distribution path is a symlink: ' + str(path))
        path.mkdir(exist_ok=True)
    specification = importlib.util.spec_from_file_location('chigiri_publish_dist', ROOT / 'scripts/publish-dist.py')
    publisher = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(publisher)
    with tempfile.TemporaryDirectory(prefix='.chigiri-' + target + '.', dir=ROOT / 'target') as temporary:
        candidate = Path(temporary) / name
        candidate.write_bytes(data)
        candidate.chmod(0o755)
        subprocess.run([sys.executable, 'scripts/verify-dist.py', '--artifact', target,
                        str(candidate)], cwd=ROOT, check=True)
        shared('verify-receipt', '--artifact', 'chigiri-cli')
        try:
            publisher.publish(candidate, slot, name, target, helper)
        except (OSError, ValueError) as error:
            raise ValueError('distribution publication failed: ' + str(error)) from error
    print('Published verified distribution candidate: target/dist/' + target + '/' + name)


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: package-distribution.py <supported-target>')
    package(sys.argv[1])
