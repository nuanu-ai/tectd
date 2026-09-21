"""Deterministic, private-safe modes for generated package trees."""
from __future__ import annotations

import os
from pathlib import Path
import stat


def normalize_package_tree(root: Path, *, root_mode: int = 0o755) -> None:
    """Normalize a generated tree without accepting links or special files."""
    root_info = root.lstat()
    if not stat.S_ISDIR(root_info.st_mode):
        raise ValueError("package root must be a directory")
    for directory, names, files in os.walk(root, topdown=True, followlinks=False):
        directory_path = Path(directory)
        directory_info = directory_path.lstat()
        if not stat.S_ISDIR(directory_info.st_mode):
            raise ValueError("package tree contains a non-directory")
        directory_path.chmod(root_mode if directory_path == root else 0o755)
        for name in names:
            path = directory_path / name
            if not stat.S_ISDIR(path.lstat().st_mode):
                raise ValueError("package tree contains a link or special directory")
        for name in files:
            path = directory_path / name
            info = path.lstat()
            if not stat.S_ISREG(info.st_mode):
                raise ValueError("package tree contains a link or special file")
            if info.st_nlink > 1:
                raise ValueError("package tree contains a hard-linked file")
            path.chmod(0o755 if info.st_mode & 0o111 else 0o644)
