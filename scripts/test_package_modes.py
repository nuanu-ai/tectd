#!/usr/bin/env python3
from __future__ import annotations

import os
from pathlib import Path
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from package_modes import normalize_package_tree


class PackageModesTest(unittest.TestCase):
    def test_modes_are_deterministic_and_safe_under_different_umasks(self) -> None:
        observed: list[dict[str, int]] = []
        for mask in (0o002, 0o077):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / "package"
                root.mkdir(mode=0o777)
                plugin = root / "codex" / "tectd"
                plugin.mkdir(parents=True)
                bridge = plugin / "tectd-mcp"
                bridge.write_bytes(b"bridge")
                run_sh = plugin / "run.sh"
                run_sh.write_text("#!/bin/sh\n")
                manifest = plugin / ".mcp.json"
                manifest.write_text("{}\n")
                for directory in (root, root / "codex", plugin):
                    directory.chmod(0o775)
                bridge.chmod(0o775)
                run_sh.chmod(0o775)
                manifest.chmod(0o664)
                previous = os.umask(mask)
                try:
                    normalize_package_tree(root)
                finally:
                    os.umask(previous)
                modes = {
                    str(path.relative_to(root)): stat.S_IMODE(path.lstat().st_mode)
                    for path in (root, root / "codex", plugin, bridge, run_sh, manifest)
                }
                observed.append(modes)
                self.assertEqual(modes["."], 0o755)
                self.assertEqual(modes["codex/tectd/tectd-mcp"], 0o755)
                self.assertEqual(modes["codex/tectd/run.sh"], 0o755)
                self.assertEqual(modes["codex/tectd/.mcp.json"], 0o644)
                archive = Path(temporary) / "package.tar"
                subprocess.run(["tar", "-cf", archive, "-C", temporary, "package"], check=True)
                with tarfile.open(archive) as bundle:
                    for member in bundle.getmembers():
                        self.assertFalse(member.mode & 0o022, member.name)
        self.assertEqual(observed[0], observed[1])

    def test_rejects_links_and_special_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "package"
            root.mkdir()
            (root / "target").write_text("data")
            (root / "link").symlink_to("target")
            with self.assertRaisesRegex(ValueError, "link or special file"):
                normalize_package_tree(root)

    def test_rejects_hard_linked_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "package"
            root.mkdir()
            target = root / "target"
            target.write_text("data")
            os.link(target, root / "hard-link")
            with self.assertRaisesRegex(ValueError, "hard-linked file"):
                normalize_package_tree(root)

    def test_preserves_private_root_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "private-output"
            root.mkdir(mode=0o700)
            child = root / "package"
            child.mkdir(mode=0o775)
            (child / "manifest.json").write_text("{}\n")
            normalize_package_tree(root, root_mode=0o700)
            self.assertEqual(stat.S_IMODE(root.lstat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(child.lstat().st_mode), 0o755)


if __name__ == "__main__":
    unittest.main()
