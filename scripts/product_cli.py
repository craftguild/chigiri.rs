"""Connect product acceptance to the actual verified engine-built CLI."""

import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def built_cli():
    subprocess.run(["sh", "tools/standards.sh", "verify-receipt", "--artifact", "chigiri-cli"],
                   cwd=ROOT, check=True)
    receipt = json.loads((ROOT / ".craftguild/chigiri/receipt.json").read_text())
    return Path(receipt["context"]["output"])


if __name__ == "__main__":
    binary = str(built_cli())
    if sys.argv[1:]:
        os.execv(binary, [binary, *sys.argv[1:]])
    print(binary)
