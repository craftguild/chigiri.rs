#!/usr/bin/env python3
"""Retain product SDK admission and emit only declared compiler environment."""

import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {'aarch64-apple-darwin', 'x86_64-apple-darwin',
           'x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'}


def tool_versions():
    values = {}
    for line in (ROOT / 'scripts/build-tool-versions.sh').read_text().splitlines():
        if not line.startswith('readonly '):
            continue
        match = re.fullmatch(r'readonly (CHIGIRI_[A-Z_]+)="([^"\n]+)"', line)
        if not match or match[1] in values:
            raise ValueError('Unsupported or duplicate immutable tool pin')
        values[match[1]] = match[2]
    return values


def context():
    target = os.environ['CRAFTGUILD_TARGET']
    host = os.environ['CRAFTGUILD_HOST']
    variant = os.environ.get('CRAFTGUILD_VARIANT', '')
    if target not in TARGETS or variant not in ('', 'distribution'):
        raise ValueError('Unsupported Chigiri build target or variant')
    distribution = variant == 'distribution' or target != host
    if distribution and os.environ['CRAFTGUILD_PROFILE'] != 'release':
        raise ValueError('Cross/distribution builds require the release profile')
    return target, distribution


def preflight():
    target, distribution = context()
    scope = target if distribution else 'native'
    subprocess.run(['sh', 'scripts/check-build-tools.sh', scope], cwd=ROOT, check=True)
    environment = {}
    if not distribution:
        environment.update(CARGO_ENCODED_RUSTFLAGS='', CARGO_ENCODED_RUSTDOCFLAGS='')
    if distribution and target.endswith('apple-darwin'):
        def xcrun(*args):
            return subprocess.check_output(['xcrun', '--sdk', 'macosx', *args],
                                           cwd=ROOT, text=True).strip()
        environment['SDKROOT'] = xcrun('--show-sdk-path')
        environment['CARGO_TARGET_' + target.upper().replace('-', '_') + '_LINKER'] = xcrun('--find', 'clang')
    elif distribution and target == 'x86_64-unknown-linux-gnu':
        environment.update(CROSS_CONFIG=str(ROOT / 'Cross.toml'), CROSS_CONTAINER_ENGINE='docker')
    destination = ROOT / '.craftguild/sdk-environment.json'
    if destination.is_symlink() or destination.parent.is_symlink():
        raise ValueError('SDK environment output cannot be a symlink')
    destination.parent.mkdir(exist_ok=True)
    destination.write_text(json.dumps(environment, indent=2) + '\n')


if __name__ == '__main__':
    preflight()
