"""Fixed isolation profile for the trusted local Codex App Server composition root.

Only configured direct MCP server IDs and plugin IDs are read from the standard
Codex user TOML. Values such as tokens, environment variables, and plugin
configuration never enter the returned launch profile or its digest.
"""

from __future__ import annotations

import hashlib
import json
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path


CODEX_EXECUTABLE = "/Users/tony/.local/bin/codex"
CODEX_CONFIG_PATH = Path("/Users/tony/.codex/config.toml")
_ALLOWED_ROUTES = {
    ("gpt-6.1-sol", "medium"),
    ("gpt-6-luna", "xhigh"),
}
_DISABLED_FEATURES = (
    "plugins",
    "remote_plugin",
    "apps",
    "shell_tool",
    "unified_exec",
    "multi_agent",
    "multi_agent_v2",
)
_CLI_ID_PATTERN = re.compile(r"[A-Za-z0-9_@-]+", re.ASCII)


class LaunchProfileError(ValueError):
    """The fixed App Server profile cannot be built safely."""


@dataclass(frozen=True)
class AppServerLaunchProfile:
    argv: tuple[str, ...]
    digest: str
    configured_mcp_count: int
    configured_plugin_count: int


def build_launch_profile(model: str = "gpt-6.1-sol", effort: str = "medium") -> AppServerLaunchProfile:
    """Build the only supported local launch profile from the standard config."""
    _validate_route(model, effort)
    try:
        config_text = CODEX_CONFIG_PATH.read_text(encoding="utf-8")
    except OSError:
        raise LaunchProfileError("cannot read the standard Codex user TOML") from None
    return _profile_from_config_text(config_text, model, effort)


def build_one_off_launch_profile() -> AppServerLaunchProfile:
    """Fixed case-only pair, never a fallback or normal route extension."""
    try:
        config_text = CODEX_CONFIG_PATH.read_text(encoding="utf-8")
    except OSError:
        raise LaunchProfileError("cannot read the standard Codex user TOML") from None
    return _one_off_profile_from_config_text(config_text)


def _one_off_profile_from_config_text(config_text: str) -> AppServerLaunchProfile:
    """Private fake TOML seam; no caller-supplied model or effort."""
    return _compose_profile(config_text, "gpt-5.6-luna", "xhigh")


def _validate_route(model: str, effort: str) -> None:
    if (model, effort) not in _ALLOWED_ROUTES:
        raise LaunchProfileError("model and effort must be one of the two current host route pairs")


def _configured_ids(config_text: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
    try:
        parsed = tomllib.loads(config_text)
    except (tomllib.TOMLDecodeError, TypeError):
        raise LaunchProfileError("standard Codex user TOML is not valid TOML") from None

    if not isinstance(parsed, dict):
        raise LaunchProfileError("standard Codex user TOML root must be a table")
    mcp_table = parsed.get("mcp_servers", {})
    plugin_table = parsed.get("plugins", {})
    if not isinstance(mcp_table, dict) or not isinstance(plugin_table, dict):
        raise LaunchProfileError("Codex MCP and plugin configuration must be tables")
    mcp_ids = tuple(sorted(mcp_table))
    plugin_ids = tuple(sorted(plugin_table))
    if any(not isinstance(item, str) or not item for item in (*mcp_ids, *plugin_ids)):
        raise LaunchProfileError("Codex MCP and plugin identifiers must be nonempty strings")
    # Codex 0.146.1 splits CLI override paths on literal periods and keeps
    # segments verbatim; TOML key quoting would create different, quoted IDs.
    if any(_CLI_ID_PATTERN.fullmatch(item) is None for item in (*mcp_ids, *plugin_ids)):
        raise LaunchProfileError("Codex MCP and plugin identifiers cannot be represented safely in CLI paths")

    # Drop parsed values immediately; only ID names survive this boundary.
    parsed.clear()
    del mcp_table, plugin_table
    return mcp_ids, plugin_ids


def _profile_from_config_text(
    config_text: str, model: str, effort: str
) -> AppServerLaunchProfile:
    """Private fake-TOML seam; production reads only ``CODEX_CONFIG_PATH``."""
    _validate_route(model, effort)
    return _compose_profile(config_text, model, effort)


def _compose_profile(config_text: str, model: str, effort: str) -> AppServerLaunchProfile:
    mcp_ids, plugin_ids = _configured_ids(config_text)

    argv: list[str] = [CODEX_EXECUTABLE, "app-server"]
    for feature in _DISABLED_FEATURES:
        argv.extend(("--disable", feature))

    for server_id in mcp_ids:
        argv.extend(("-c", f"mcp_servers.{server_id}.enabled=false"))
    for plugin_id in plugin_ids:
        argv.extend(("-c", f"plugins.{plugin_id}.enabled=false"))

    # These explicit compatibility overrides suppress config tables that are
    # incompatible with the current local CLI parser. The user config is never edited.
    for override in (
        "features.context_management=false",
        "features.multi_agent_v2=false",
        'web_search="disabled"',
        f'model={json.dumps(model)}',
        f'model_reasoning_effort={json.dumps(effort)}',
    ):
        argv.extend(("-c", override))

    frozen_argv = tuple(argv)
    canonical = json.dumps(frozen_argv, ensure_ascii=False, separators=(",", ":"))
    digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return AppServerLaunchProfile(
        argv=frozen_argv,
        digest=digest,
        configured_mcp_count=len(mcp_ids),
        configured_plugin_count=len(plugin_ids),
    )
