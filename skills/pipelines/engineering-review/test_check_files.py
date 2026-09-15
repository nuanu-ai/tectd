import json
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


HELPER = Path(__file__).with_name("check_files.py")


class CheckFilesTest(unittest.TestCase):
    def invoke(self, root, files):
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps({"files": files}))
        return subprocess.run(
            [sys.executable, HELPER, "--repo-root", root, "--manifest", manifest],
            text=True, capture_output=True, check=False,
        )

    def run_check(self, lines: int):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "declarations.rs").write_text("x\n" * lines)
            return self.invoke(root, [{
                "path": "declarations.rs",
                "content_kind": "declarative",
                "responsibility": "One declarative definition set.",
                "justification": "Splitting would separate one definition set."
            }])

    def test_accepts_exactly_1500_physical_lines(self):
        result = self.run_check(1500)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["files"][0]["line_count"], 1500)

    def test_rejects_1501_physical_lines(self):
        result = self.run_check(1501)
        self.assertEqual(result.returncode, 2)
        self.assertIn("exceeds 1500", result.stderr)

    def test_counts_empty_and_terminal_newline_without_phantom_line(self):
        spec = importlib.util.spec_from_file_location("check_files", HELPER)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.assertEqual(module.physical_lines(b""), 0)
        self.assertEqual(module.physical_lines(b"one\n"), 1)
        self.assertEqual(module.physical_lines(b"one"), 1)

    def test_rejects_malformed_escape_symlink_and_count_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "safe.rs").write_text("x\n")
            outside = root.parent / f"{root.name}-outside.rs"
            outside.write_text("x\n")
            (root / "link.rs").symlink_to(outside)
            base = {"content_kind": "behavioral", "responsibility": "Owner."}
            cases = [
                [{"path": "safe.rs", "content_kind": "behavioral", "responsibility": " "}],
                [{"path": "../outside.rs", **base}],
                [{"path": "link.rs", **base}],
                [{"path": "safe.rs", **base} for _ in range(101)],
            ]
            for files in cases:
                self.assertEqual(self.invoke(root, files).returncode, 2)
            outside.unlink()

    def test_rejects_oversized_single_line_without_buffering_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "large.rs").write_bytes(b"x" * (16 * 1024 * 1024 + 1))
            result = self.invoke(root, [{
                "path":"large.rs","content_kind":"declarative",
                "responsibility":"One declaration.",
            }])
            self.assertEqual(result.returncode, 2)
            self.assertIn("file exceeds", result.stderr)


if __name__ == "__main__":
    unittest.main()
