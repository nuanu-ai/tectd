"""Conservative Rust source reachability for the architecture file-size gate.

Only a file reached exclusively from an integration-test root or a literal
``#[cfg(test)] mod`` edge is test-only. Unknown files remain production-capped.
"""

from collections import deque
from pathlib import Path
import re
import tomllib

MODULE = re.compile(
    r"(?m)^[ \t]*(?P<attrs>(?:#\[[^\]\n]+\][ \t]*\n[ \t]*)*)"
    r"(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+(?P<name>[A-Za-z_]\w*)[ \t]*;"
)
TEST_CFG = re.compile(r"#\[[ \t]*cfg[ \t]*\([ \t]*test[ \t]*\)[ \t]*\]")
PATH_ATTR = re.compile(r'#\[[ \t]*path[ \t]*=[ \t]*"([^"\n]+)"[ \t]*\]')
INCLUDE = re.compile(r'\binclude![ \t]*\([ \t]*"([^"\n]+)"[ \t]*\)')
ANY_INCLUDE = re.compile(r"\binclude![ \t]*\(")
INLINE_MODULE = re.compile(r"\bmod[ \t]+[A-Za-z_]\w*[ \t]*\{")


def _module_file(source: Path, name: str, explicit: str | None) -> Path | None:
    if explicit is not None:
        candidate = (source.parent / explicit).resolve()
        return candidate if candidate.is_file() else None
    directory = source.parent if source.stem in {"lib", "main", "mod"} else source.with_suffix("")
    for candidate in (directory / f"{name}.rs", directory / name / "mod.rs"):
        if candidate.is_file():
            return candidate.resolve()
    return None


def _edges(source: Path) -> tuple[list[tuple[Path, bool]], bool]:
    text = source.read_text()
    edges = []
    for match in MODULE.finditer(text):
        attrs = match.group("attrs")
        explicit = PATH_ATTR.search(attrs)
        target = _module_file(source, match.group("name"), explicit.group(1) if explicit else None)
        if target is not None:
            edges.append((target, TEST_CFG.search(attrs) is not None))
        if explicit is None and INLINE_MODULE.search(text[: match.start()]):
            # An external module can occur inside an inline module. Search all
            # possible nested module directories and classify ambiguous edges
            # as production-reachable rather than granting an exemption.
            base = source.parent if source.stem in {"lib", "main", "mod"} else source.with_suffix("")
            for nested in base.rglob(f"{match.group('name')}.rs") if base.is_dir() else ():
                edges.append((nested.resolve(), False))
            for nested in base.rglob(f"{match.group('name')}/mod.rs") if base.is_dir() else ():
                edges.append((nested.resolve(), False))
    for match in INCLUDE.finditer(text):
        target = (source.parent / match.group(1)).resolve()
        if target.is_file() and target.suffix == ".rs":
            # A lexical include inherits the caller's production/test context.
            edges.append((target, False))
    return edges, len(ANY_INCLUDE.findall(text)) != len(INCLUDE.findall(text))


def test_only_sources(root: Path) -> set[Path]:
    """Prove test-only reachability; an unvisited or mixed source is capped."""
    roots: deque[tuple[Path, bool]] = deque()
    for manifest in (root / "crates").glob("*/Cargo.toml"):
        crate = manifest.parent
        data = tomllib.loads(manifest.read_text())
        default_lib = crate / "src/lib.rs"
        if default_lib.is_file():
            roots.append((default_lib.resolve(), False))
        lib_path = data.get("lib", {}).get("path")
        if lib_path:
            roots.append(((crate / lib_path).resolve(), False))
        for default_bin in (crate / "src/bin").glob("*.rs"):
            roots.append((default_bin.resolve(), False))
        default_main = crate / "src/main.rs"
        if default_main.is_file():
            roots.append((default_main.resolve(), False))
        for target in data.get("bin", []):
            if "path" in target:
                roots.append(((crate / target["path"]).resolve(), False))
        if data.get("package", {}).get("autotests", True):
            for integration in (crate / "tests").glob("*.rs"):
                roots.append((integration.resolve(), True))
        for target in data.get("test", []):
            if "path" in target:
                roots.append(((crate / target["path"]).resolve(), True))

    production: set[Path] = set()
    tests: set[Path] = set()
    unknown_include = False
    while roots:
        source, test_context = roots.popleft()
        if not source.is_file():
            continue
        seen = tests if test_context else production
        if source in seen:
            continue
        seen.add(source)
        edges, unresolved_include = _edges(source)
        unknown_include |= unresolved_include
        for target, gated in edges:
            roots.append((target, test_context or gated))
    return set() if unknown_include else tests - production


def over_limit_sources(root: Path, limit: int = 500) -> list[tuple[Path, int]]:
    exempt = test_only_sources(root)
    violations = []
    for source in sorted((root / "crates").rglob("*")):
        if not source.is_file() or source.suffix not in {".rs", ".sql"}:
            continue
        lines = len(source.read_text().splitlines())
        if lines > limit and (source.suffix == ".sql" or source.resolve() not in exempt):
            violations.append((source, lines))
    return violations
