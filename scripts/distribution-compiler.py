#!/usr/bin/env python3
"""Adapt selected common compiler argv to pinned Cross or isolated cargo-xwin."""

import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

from sdk_preflight import ROOT, context, tool_versions


def run_xwin(args, target):
    pins = tool_versions()
    cache = os.environ.get('CHIGIRI_XWIN_CACHE_DIR') or str(
        ROOT / 'target/tool-cache' / (
            'cargo-xwin-' + pins['CHIGIRI_CARGO_XWIN_VERSION'] +
            '-sdk-' + pins['CHIGIRI_WINDOWS_SDK_VERSION'] +
            '-crt-' + pins['CHIGIRI_WINDOWS_CRT_VERSION'] +
            '-vs-' + pins['CHIGIRI_XWIN_VERSION'] + '-' + pins['CHIGIRI_XWIN_VARIANT']))
    source = Path(cache)
    if not source.is_absolute():
        source = ROOT / source
    subprocess.run(['cargo', 'fetch', '--locked', '--offline', '--target', target], cwd=ROOT, check=True)
    base = ROOT / '.craftguild/windows-sdk-views'
    if base.is_symlink():
        raise ValueError('Windows SDK view parent cannot be a symlink')
    base.mkdir(parents=True, exist_ok=True)
    view = Path(tempfile.mkdtemp(prefix='build-', dir=base))
    view.rmdir()  # The existing no-follow stage operation creates the fresh view.
    command = [sys.executable, str(ROOT / 'scripts/xwin-cache.py')]
    try:
        subprocess.run([*command, 'stage', str(source), str(view),
                        pins['CHIGIRI_CARGO_XWIN_VERSION'], pins['CHIGIRI_XWIN_VERSION'],
                        pins['CHIGIRI_WINDOWS_SDK_VERSION'], pins['CHIGIRI_WINDOWS_CRT_VERSION'],
                        'x86_64', pins['CHIGIRI_XWIN_VARIANT']], cwd=ROOT, check=True)
        subprocess.run([*command, 'verify-view', str(view)], cwd=ROOT, check=True)
        env = dict(os.environ, CARGO_NET_OFFLINE='true', XWIN_CACHE_DIR=str(view),
                   XWIN_ARCH='x86_64', XWIN_VERSION=pins['CHIGIRI_XWIN_VERSION'],
                   XWIN_SDK_VERSION=pins['CHIGIRI_WINDOWS_SDK_VERSION'],
                   XWIN_CRT_VERSION=pins['CHIGIRI_WINDOWS_CRT_VERSION'],
                   XWIN_VARIANT=pins['CHIGIRI_XWIN_VARIANT'], XWIN_CROSS_COMPILER='clang-cl',
                   XWIN_INCLUDE_ATL='false', XWIN_INCLUDE_DEBUG_LIBS='false',
                   XWIN_INCLUDE_DEBUG_SYMBOLS='false', XWIN_HTTP_RETRIES='0')
        host = os.environ['CRAFTGUILD_HOST']
        if host.endswith('apple-darwin'):
            guard = ['sandbox-exec', '--deny-network']
        elif host.endswith('unknown-linux-gnu'):
            guard = ['unshare', '--user-network']
        else:
            raise ValueError('No qualified Windows build network guard for this host')
        subprocess.run(['sh', 'scripts/run-without-network.sh', *guard,
                        'cargo', 'xwin', *args], cwd=ROOT, env=env, check=True)
        subprocess.run([*command, 'verify-view', str(view)], cwd=ROOT, check=True)
    finally:
        if view.exists():
            subprocess.run([*command, 'unseal-view', str(view)], cwd=ROOT, check=True)
            shutil.rmtree(view)


def main():
    args = sys.argv[1:]
    target, distribution = context()
    if not distribution or not args or args[0] != 'build' or args.count('--target') != 1:
        raise ValueError('Expected one engine-selected distribution build target')
    if args[args.index('--target') + 1] != target:
        raise ValueError('Compiler argv differs from the engine target')
    if target == 'x86_64-pc-windows-msvc':
        def terminate(number, _frame):
            raise SystemExit(128 + number)
        signal.signal(signal.SIGTERM, terminate)
        run_xwin(args, target)
    elif target == 'x86_64-unknown-linux-gnu':
        os.execvp('cross', ['cross', *args])
    else:
        os.execvp('cargo', ['cargo', *args])


if __name__ == '__main__':
    main()
