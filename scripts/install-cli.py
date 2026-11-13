#!/usr/bin/env python3
"""Compatibility entry for shared copy-only install and its recorded selectors."""
import os
from pathlib import Path
import sys

root = Path(__file__).resolve().parent.parent
os.execvp('sh', ['sh', str(root / 'tools/standards.sh'), 'install',
                  '--artifact', 'chigiri-cli', *sys.argv[1:]])
