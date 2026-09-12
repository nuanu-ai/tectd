#!/usr/bin/env python3
"""Package the independent TectD MCP plugin without installing it."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path,
                        help="New parent directory; creates its tectd child")
    args = parser.parse_args()
    binary = args.binary.expanduser().resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error("--binary must be an executable file")
    output = args.output.expanduser().absolute()
    if output.exists():
        parser.error("--output must not already exist")
    template = Path(__file__).resolve().parents[1] / "integrations/codex/tectd"
    output.mkdir(parents=True, exist_ok=False)
    package = output / "tectd"
    shutil.copytree(template, package)
    (package / "bin").mkdir()
    target = package / "bin/tectd-mcp"
    shutil.copy2(binary, target)
    manifest = {
        "plugin": "tectd", "display_name": "TectD MCP", "mcp_server": "tectd",
        "binary_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
        "skill_delivery": "tectd-program and tectd-setup are embedded in the binary and served by read_skill",
        "installation_performed": False,
    }
    (output / "package-proof.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(package)


if __name__ == "__main__":
    main()
