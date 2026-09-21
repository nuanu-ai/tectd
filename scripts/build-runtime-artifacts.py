#!/usr/bin/env python3
"""Build pinned Linux TectD artifacts and the pgRDF PostgreSQL image."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import urllib.request

from package_modes import normalize_package_tree


RUST_IMAGE = "rust:1.93.0-bookworm@sha256:7274e0edb5b47eda8053b350ebf3d489f7e0f65d2d7e77b16076299c7c047c28"
RUNTIME_IMAGE = "debian:bookworm-slim@sha256:5ae3c39ebd15e229dcedd5cee596b2497182493d41ff162e824ba13fc1b2b867"
POSTGRES_IMAGE = "postgres:18.6-trixie@sha256:7341002d2b8c7c5bdd7542a671a95b36196c0b5b888daf454ae4fc33ba5346d7"
PGRDF_URL = "https://github.com/styk-tv/pgRDF/releases/download/v0.6.34/pgrdf-0.6.34-pg18-glibc-amd64.tar.gz"
PGRDF_NAME = "pgrdf-0.6.34-pg18-glibc-amd64.tar.gz"
PGRDF_SIZE = 10_588_401
PGRDF_SHA256 = "78b9efa983bc5b494f163a7896cd2260314830ac166e8ab2b04e549f91852b8f"
PGRDF_COMMIT = "1b82842dcb60b74c060b0344310b47d1c84a8e8d"
PGRDF_GHCR_LEAF = "sha256:68e669adec74d38532c012d2ea1a7d677c95fa2856f96671d0e1cacf15b2e1b2"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
TAG = re.compile(r"^tectd-(?:runtime|postgres-pgrdf):[a-z0-9][a-z0-9._-]{0,63}$")


def run(argv: list[str], *, cwd: Path | None = None) -> str:
    result = subprocess.run(argv, cwd=cwd, check=True, text=True, stdout=subprocess.PIPE)
    return result.stdout.strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def download_asset(destination: Path) -> None:
    request = urllib.request.Request(PGRDF_URL, headers={"User-Agent": "tectd-artifact-builder/1"})
    with urllib.request.urlopen(request, timeout=120) as source, destination.open("xb") as target:
        shutil.copyfileobj(source, target)
    if destination.stat().st_size != PGRDF_SIZE or sha256(destination) != PGRDF_SHA256:
        raise SystemExit("pgRDF archive size or digest mismatch")


def validate_archive(archive: Path, extraction: Path) -> dict[str, object]:
    with tarfile.open(archive, "r:gz") as bundle:
        members = bundle.getmembers()
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk():
                raise SystemExit(f"unsafe pgRDF archive member: {member.name}")
        bundle.extractall(extraction, filter="data")
    roots = [path for path in extraction.iterdir() if path.is_dir()]
    if len(roots) != 1:
        raise SystemExit("pgRDF archive must contain one root directory")
    root = roots[0]
    sums = (root / "SHA256SUMS").read_text().splitlines()
    checked: list[str] = []
    for line in sums:
        expected, relative = line.split(maxsplit=1)
        relative = relative.removeprefix("./")
        target = root / relative
        if not HEX64.fullmatch(expected) or not target.is_file() or sha256(target) != expected:
            raise SystemExit(f"pgRDF internal digest mismatch: {relative}")
        checked.append(relative)
    manifest = json.loads((root / "MANIFEST.json").read_text())
    expected_manifest = {
        "version": "0.6.34",
        "extversion": "0.6.34",
    }
    for key, expected in expected_manifest.items():
        if manifest.get(key) != expected:
            raise SystemExit(f"unexpected pgRDF manifest {key}")
    if manifest.get("build", {}).get("git_sha") != PGRDF_COMMIT:
        raise SystemExit("unexpected pgRDF source commit")
    platform = manifest.get("platform", {})
    if platform.get("pg_major") != "18" or platform.get("arch") != "amd64":
        raise SystemExit("unexpected pgRDF platform")
    library = root / "lib/pgrdf.so"
    header = library.read_bytes()[:20]
    if header[:5] != b"\x7fELF\x02" or int.from_bytes(header[18:20], "little") != 62:
        raise SystemExit("pgRDF library is not ELF64 amd64")
    glibc_versions = sorted(
        {tuple(map(int, match.split(b"."))) for match in re.findall(rb"GLIBC_([0-9]+\.[0-9]+)", library.read_bytes())}
    )
    return {
        "members": len(members),
        "internal_files_verified": checked,
        "manifest": manifest,
        "elf_machine": "EM_X86_64",
        "glibc_symbols": [".".join(map(str, version)) for version in glibc_versions],
        "glibc_floor": ".".join(map(str, glibc_versions[-1])),
        "library_sha256": sha256(library),
    }


def elf_evidence(path: Path) -> dict[str, object]:
    data = path.read_bytes()
    if data[:5] != b"\x7fELF\x02" or int.from_bytes(data[18:20], "little") != 62:
        raise SystemExit(f"not an ELF64 amd64 binary: {path.name}")
    versions = sorted(
        {tuple(map(int, match.split(b"."))) for match in re.findall(rb"GLIBC_([0-9]+\.[0-9]+)", data)}
    )
    return {
        "sha256": sha256(path),
        "size": path.stat().st_size,
        "elf_class": "ELF64",
        "elf_machine": "EM_X86_64",
        "glibc_symbols": [".".join(map(str, version)) for version in versions],
        "glibc_max_required": ".".join(map(str, versions[-1])) if versions else None,
    }


def docker(builder: str, arguments: list[str], *, cwd: Path | None = None) -> str:
    return run(["docker", "buildx", "build", "--builder", builder, "--platform", "linux/amd64", *arguments], cwd=cwd)


def refuse_existing_image(tag: str) -> None:
    result = subprocess.run(
        ["docker", "image", "inspect", tag],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode == 0:
        raise SystemExit(f"refusing to overwrite existing image tag: {tag}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--source-tree", required=True)
    parser.add_argument("--builder", required=True)
    parser.add_argument("--runtime-tag", required=True)
    parser.add_argument("--postgres-tag", required=True)
    args = parser.parse_args()

    if not args.source.is_absolute():
        parser.error("--source must be an absolute path")
    if not args.output.is_absolute():
        parser.error("--output must be an absolute path")
    source = args.source.resolve(strict=True)
    output = args.output.resolve(strict=False)
    if output == source or output.is_relative_to(source):
        parser.error("--output must be outside the source tree")
    if output.exists():
        parser.error("--output must be a new absolute path")
    if not HEX40.fullmatch(args.source_commit) or not HEX40.fullmatch(args.source_tree):
        parser.error("source commit and tree must be lowercase 40-character hashes")
    if not re.fullmatch(r"tectd-runtime-[a-z0-9-]{8,64}", args.builder):
        parser.error("unexpected builder name")
    if not TAG.fullmatch(args.runtime_tag) or not args.runtime_tag.startswith("tectd-runtime:"):
        parser.error("unexpected runtime tag")
    if not TAG.fullmatch(args.postgres_tag) or not args.postgres_tag.startswith("tectd-postgres-pgrdf:"):
        parser.error("unexpected PostgreSQL tag")
    refuse_existing_image(args.runtime_tag)
    refuse_existing_image(args.postgres_tag)

    runtime_recipe = source / "containers/runtime/Dockerfile"
    postgres_recipe = source / "containers/postgres/Dockerfile"
    package_helper = source / "scripts/package-codex-plugin.py"
    package_modes = source / "scripts/package_modes.py"
    for required in (runtime_recipe, postgres_recipe, package_helper, package_modes, source / "Cargo.lock"):
        if not required.is_file():
            parser.error(f"missing source input: {required.relative_to(source)}")

    output.mkdir(parents=True, mode=0o700)
    downloads = output / "downloads"
    extracted = output / "pgrdf-inspection"
    binaries = output / "tect-binaries"
    postgres_context = output / "postgres-context"
    downloads.mkdir()
    extracted.mkdir()
    postgres_context.mkdir()
    archive = downloads / PGRDF_NAME
    download_asset(archive)
    pgrdf = validate_archive(archive, extracted)

    docker(
        args.builder,
        ["--file", str(runtime_recipe), "--target", "artifacts", "--output", f"type=local,dest={binaries}", str(source)],
    )
    docker(
        args.builder,
        ["--file", str(runtime_recipe), "--target", "runtime", "--tag", args.runtime_tag, "--load", str(source)],
    )
    shutil.copy2(postgres_recipe, postgres_context / "Dockerfile")
    shutil.copy2(archive, postgres_context / PGRDF_NAME)
    docker(
        args.builder,
        ["--file", str(postgres_context / "Dockerfile"), "--tag", args.postgres_tag, "--load", str(postgres_context)],
    )

    binary_evidence = {name: elf_evidence(binaries / name) for name in ("tectd", "tectd-mcp", "tect-admin")}
    for name in binary_evidence:
        os.chmod(binaries / name, 0o755)
    package = output / "package"
    package.mkdir()
    package_bin = package / "bin"
    package_bin.mkdir()
    for name in binary_evidence:
        shutil.copy2(binaries / name, package_bin / name)
    run([str(package_helper), "--binary", str(binaries / "tectd-mcp"), "--output", str(package / "codex")], cwd=source)

    runtime_id = run(["docker", "image", "inspect", args.runtime_tag, "--format", "{{.Id}}"])
    postgres_id = run(["docker", "image", "inspect", args.postgres_tag, "--format", "{{.Id}}"])
    packages = run([
        "docker", "run", "--rm", "--entrypoint", "dpkg-query", args.runtime_tag,
        "-W", "-f=${Package}=${Version}\\n", "git", "ca-certificates", "libc6",
    ]).splitlines()
    runtime_probe = {
        "tect_admin_help": run(["docker", "run", "--rm", "--entrypoint", "/usr/local/bin/tect-admin", args.runtime_tag, "--help"]).splitlines()[0],
        "git_version": run(["docker", "run", "--rm", "--entrypoint", "git", args.runtime_tag, "--version"]),
        "user": run(["docker", "run", "--rm", "--entrypoint", "id", args.runtime_tag, "-u"]),
        "packages": packages,
    }
    if runtime_probe["user"] != "10001":
        raise SystemExit("runtime image user mismatch")

    package_proof_path = package / "codex/package-proof.json"
    package_proof = json.loads(package_proof_path.read_text())
    package_proof.update({
        "source_commit": args.source_commit,
        "source_tree": args.source_tree,
        "platform": "linux/amd64",
        "binaries": binary_evidence,
        "runtime_image_id": runtime_id,
    })
    package_proof_path.write_text(json.dumps(package_proof, indent=2, sort_keys=True) + "\n")
    shutil.copy2(package_proof_path, package / "package-proof.json")
    normalize_package_tree(package)

    archive_path = output / "tectd-runtime-linux-amd64.tar.gz"
    run([
        "tar", "--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner",
        "-czf", str(archive_path), "-C", str(output), "package",
    ])
    recipes = {
        "containers/runtime/Dockerfile": sha256(runtime_recipe),
        "containers/postgres/Dockerfile": sha256(postgres_recipe),
        "scripts/build-runtime-artifacts.py": sha256(Path(__file__).resolve()),
        "scripts/package-codex-plugin.py": sha256(package_helper),
        "scripts/package_modes.py": sha256(package_modes),
    }
    manifest = {
        "schema_version": 1,
        "source": {"commit": args.source_commit, "tree": args.source_tree},
        "platform": "linux/amd64",
        "pins": {
            "rust_builder": RUST_IMAGE,
            "runtime_base": RUNTIME_IMAGE,
            "postgres_base": POSTGRES_IMAGE,
            "pgrdf_url": PGRDF_URL,
            "pgrdf_size": PGRDF_SIZE,
            "pgrdf_sha256": PGRDF_SHA256,
            "pgrdf_source_commit": PGRDF_COMMIT,
            "pgrdf_ghcr_leaf": PGRDF_GHCR_LEAF,
        },
        "recipes": recipes,
        "binaries": binary_evidence,
        "pgrdf": pgrdf,
        "images": {
            "runtime": {"tag": args.runtime_tag, "id": runtime_id},
            "postgres": {"tag": args.postgres_tag, "id": postgres_id},
        },
        "runtime_probe": runtime_probe,
        "package": {"path": archive_path.name, "size": archive_path.stat().st_size, "sha256": sha256(archive_path)},
    }
    (output / "artifact-manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"output": str(output), "runtime_image_id": runtime_id, "postgres_image_id": postgres_id}, sort_keys=True))


if __name__ == "__main__":
    main()
