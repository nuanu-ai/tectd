"""Unit tests for fixed Codex App Server isolation using fake TOML only."""

import json
import re
import unittest

from scripts.codex_app_server_profile import (
    AppServerLaunchProfile,
    LaunchProfileError,
    _profile_from_config_text,
)


MODEL = "gpt-6.1-sol"
EFFORT = "medium"
SECRET = "fake-secret-must-never-escape"


CONFIG = f'''api_key = "{SECRET}"

[mcp_servers.alpha]
enabled = true
token = "{SECRET}"

[mcp_servers."server-with-hyphen"]
command = "safe-fake"

[plugins."plugin@bundle"]
enabled = true
secret = "{SECRET}"

[plugins."plugin_id@other-bundle"]
enabled = true
'''


def config_with_ids(mcp_ids, plugin_ids, secret):
    lines = [f'api_key = "{secret}"']
    for server_id in mcp_ids:
        lines.extend((f"[mcp_servers.{json.dumps(server_id)}]", f'token = "{secret}"'))
    for plugin_id in plugin_ids:
        lines.extend((f"[plugins.{json.dumps(plugin_id)}]", f'secret = "{secret}"'))
    return "\n".join(lines) + "\n"


class AppServerProfileTests(unittest.TestCase):
    def test_disables_every_direct_id_with_actual_cli_literal_path_semantics(self):
        original = CONFIG
        profile = _profile_from_config_text(CONFIG, MODEL, EFFORT)
        self.assertIs(type(profile), AppServerLaunchProfile)
        self.assertEqual(profile.configured_mcp_count, 2)
        self.assertEqual(profile.configured_plugin_count, 2)
        # Match the installed CLI's path.split('.') and verbatim segments,
        # rather than interpreting these CLI strings as TOML dotted keys.
        disabled_ids = {"mcp_servers": set(), "plugins": set()}
        for argument in profile.argv:
            if argument.startswith(("mcp_servers.", "plugins.")):
                path, value = argument.split("=", 1)
                section, identifier, field = path.split(".")
                self.assertEqual(field, "enabled")
                self.assertEqual(value, "false")
                disabled_ids[section].add(identifier)
                self.assertNotIn('"', identifier)
        self.assertEqual(disabled_ids["mcp_servers"], {"alpha", "server-with-hyphen"})
        self.assertEqual(disabled_ids["plugins"], {"plugin@bundle", "plugin_id@other-bundle"})
        self.assertEqual(CONFIG, original)
        self.assertNotIn(SECRET, repr(profile.argv))
        self.assertRegex(profile.digest, re.compile(r"^[0-9a-f]{64}$"))

    def test_profile_is_order_stable_and_secret_values_do_not_affect_digest(self):
        first = config_with_ids(["z_server", "a-server"], ["z@bundle", "a@bundle"], "secret-one")
        second = config_with_ids(["a-server", "z_server"], ["a@bundle", "z@bundle"], "secret-two")
        profile_one = _profile_from_config_text(first, MODEL, EFFORT)
        profile_two = _profile_from_config_text(second, MODEL, EFFORT)
        self.assertEqual(profile_one.argv, profile_two.argv)
        self.assertEqual(profile_one.digest, profile_two.digest)
        self.assertNotIn("secret-one", repr(profile_one))
        self.assertNotIn("secret-two", repr(profile_two))

    def test_unrepresentable_ids_fail_closed_without_echoing_ids_or_values(self):
        for identifier in ("server.with.dot", 'quoted"id', "line\nbreak", "space id",
                           "id=value", "slash/id", "unicode-\u00e9"):
            for section in ("mcp_servers", "plugins"):
                with self.subTest(identifier=identifier, section=section):
                    config = config_with_ids(
                        [identifier] if section == "mcp_servers" else ["safe-server"],
                        [identifier] if section == "plugins" else ["safe@bundle"],
                        SECRET,
                    )
                    with self.assertRaisesRegex(LaunchProfileError, "cannot be represented safely") as raised:
                        _profile_from_config_text(config, MODEL, EFFORT)
                    self.assertNotIn(identifier, str(raised.exception))
                    self.assertNotIn(SECRET, str(raised.exception))

    def test_empty_id_fails_closed(self):
        with self.assertRaisesRegex(LaunchProfileError, "nonempty strings"):
            _profile_from_config_text(config_with_ids([""], [], SECRET), MODEL, EFFORT)

    def test_route_pairs_are_closed_and_other_values_are_rejected(self):
        for model, effort in (("gpt-6.1-sol", "xhigh"), ("gpt-6-luna", "medium"),
                              ("unknown", "medium"), ("gpt-6.1-sol", "")):
            with self.subTest(model=model, effort=effort), self.assertRaises(LaunchProfileError):
                _profile_from_config_text(CONFIG, model, effort)
        luna = _profile_from_config_text(CONFIG, "gpt-6-luna", "xhigh")
        self.assertIn('model="gpt-6-luna"', luna.argv)
        self.assertIn('model_reasoning_effort="xhigh"', luna.argv)

    def test_required_isolation_flags_and_compatibility_overrides_are_present(self):
        profile = _profile_from_config_text(CONFIG, MODEL, EFFORT)
        adjacent_arguments = tuple(zip(profile.argv, profile.argv[1:]))
        for feature in ("plugins", "remote_plugin", "apps", "shell_tool", "unified_exec",
                        "multi_agent", "multi_agent_v2"):
            self.assertIn(("--disable", feature), adjacent_arguments)
        for override in (
            "features.context_management=false",
            "features.multi_agent_v2=false",
            'web_search="disabled"',
            'model="gpt-6.1-sol"',
            'model_reasoning_effort="medium"',
        ):
            self.assertIn(override, profile.argv)

    def test_invalid_toml_or_non_table_sections_fail_without_echoing_contents(self):
        with self.assertRaisesRegex(LaunchProfileError, "not valid TOML"):
            _profile_from_config_text(f'api_key = "{SECRET}\n', MODEL, EFFORT)
        with self.assertRaisesRegex(LaunchProfileError, "must be tables"):
            _profile_from_config_text('mcp_servers = "wrong-shape"', MODEL, EFFORT)


if __name__ == "__main__":
    unittest.main()
