#!/usr/bin/env python3
"""Create and remove one isolated canonical PostgreSQL CI fixture."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid


def run(*argv, check=True):
    return subprocess.run(argv, check=check, text=True, stdout=subprocess.PIPE).stdout.strip()


def save(path, state):
    path.write_text(json.dumps(state, indent=2) + "\n")


def start(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        raise SystemExit("refusing to overwrite fixture state")
    token = uuid.uuid4().hex
    name = "tectd-ci-pg-" + token
    state = {"token": token, "container": name, "network": name, "volume": name,
             "image": name + ":fixture", "build_route": "Docker legacy builder (deprecated)"}
    save(path, state)
    downloads = path.parent / (name + "-build")
    downloads.mkdir(mode=0o700)
    spec = importlib.util.spec_from_file_location("canonical_builder", Path(__file__).with_name("build-runtime-artifacts.py"))
    canonical = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(canonical)
    archive = downloads / canonical.PGRDF_NAME
    canonical.download_asset(archive)
    inspection = downloads / "inspection"
    inspection.mkdir()
    state["archive"] = canonical.validate_archive(archive, inspection)
    state["archive_sha256"] = canonical.sha256(archive)
    save(path, state)
    recipe = Path(__file__).resolve().parent.parent / "containers/postgres/Dockerfile"
    # The canonical recipe needs no BuildKit features; keep build resources bounded.
    subprocess.run(["docker", "build", "--platform", "linux/amd64", "--memory", "256m",
                    "--memory-swap", "256m", "--cpu-period", "100000", "--cpu-quota", "25000",
                    "--force-rm", "--label", "tectd.ci.fixture=" + token, "--tag", state["image"],
                    "--file", str(recipe), str(downloads)], check=True,
                   env={**os.environ, "DOCKER_BUILDKIT": "0"})
    run("docker", "network", "create", "--label", "tectd.ci.fixture=" + token, name)
    run("docker", "volume", "create", "--label", "tectd.ci.fixture=" + token, name)
    run("docker", "run", "--detach", "--name", name, "--label", "tectd.ci.fixture=" + token,
        "--network", name, "--memory", "512m", "--memory-swap", "512m", "--cpus", "0.25",
        "--publish", "127.0.0.1::5432", "--mount", "type=volume,source=" + name + ",target=/var/lib/postgresql",
        "--env", "POSTGRES_PASSWORD=postgres", "--env", "POSTGRES_DB=tect_test",
        "--health-cmd", "pg_isready -U postgres -d tect_test", "--health-interval", "5s",
        "--health-timeout", "5s", "--health-retries", "12", state["image"])
    for _ in range(60):
        health = run("docker", "inspect", "--format", "{{.State.Health.Status}}", name)
        if health == "healthy":
            break
        if health == "unhealthy":
            raise SystemExit("canonical PostgreSQL failed health check")
        time.sleep(1)
    else:
        raise SystemExit("canonical PostgreSQL health deadline exceeded")
    run("docker", "exec", name, "psql", "-U", "postgres", "-d", "tect_test", "-v", "ON_ERROR_STOP=1",
        "-c", "CREATE ROLE tect_ci LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT PASSWORD 'ci-fixture-only'")
    port = run("docker", "port", name, "5432/tcp")
    if not port.startswith("127.0.0.1:") or "\n" in port:
        raise SystemExit("fixture port is not confined to loopback")
    state["port"] = int(port.split(":")[1])
    state["identity"] = run("docker", "exec", name, "psql", "-U", "postgres", "-d", "tect_test", "-Atc",
        "SELECT version(); SHOW shared_preload_libraries; SELECT default_version FROM pg_available_extensions WHERE name='pgrdf'; "
        "SELECT rolname,rolsuper,rolbypassrls,rolinherit FROM pg_roles WHERE rolname='tect_ci'; SELECT count(*) FROM pg_auth_members WHERE member=(SELECT oid FROM pg_roles WHERE rolname='tect_ci')")
    save(path, state)
    if "pgrdf\n0.6.34\ntect_ci|f|f|f\n0" not in state["identity"]:
        raise SystemExit("canonical fixture identity or role mismatch")
    env = {"TECT_TEST_ADMIN_URL": f"postgres://postgres:postgres@127.0.0.1:{state['port']}/tect_test",
           "TECT_TEST_RUNTIME_URL": f"postgres://tect_ci:ci-fixture-only@127.0.0.1:{state['port']}/tect_test"}
    if os.environ.get("GITHUB_ENV"):
        with open(os.environ["GITHUB_ENV"], "a") as output:
            for key, value in env.items():
                output.write(key + "=" + value + "\n")
    print(json.dumps({"state": str(path), "identity": state["identity"], "port": state["port"]}))


def cleanup(path):
    if not path.exists():
        return
    state = json.loads(path.read_text())
    token = state["token"]
    if len(token) != 32 or any(c not in "0123456789abcdef" for c in token):
        raise SystemExit("invalid fixture ownership token")
    name = "tectd-ci-pg-" + token
    for kind in ("container", "network", "volume", "image"):
        expected = name + ":fixture" if kind == "image" else name
        if state[kind] != expected:
            raise SystemExit("fixture resource name mismatch")
        result = subprocess.run(["docker", kind, "inspect", expected], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        if result.returncode:
            continue
        item = json.loads(result.stdout)[0]
        labels = item.get("Config", {}).get("Labels", {}) if kind in ("container", "image") else item.get("Labels", {})
        if labels.get("tectd.ci.fixture") != token:
            raise SystemExit("refusing to remove resource with different ownership label")
        command = ["docker", kind, "rm"]
        if kind == "container":
            command.append("--force")
        run(*command, expected)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("start", "cleanup"))
    parser.add_argument("state", type=Path)
    args = parser.parse_args()
    sys.dont_write_bytecode = True
    (start if args.action == "start" else cleanup)(args.state.resolve())
