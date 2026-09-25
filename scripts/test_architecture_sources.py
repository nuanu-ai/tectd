"""Fail-closed fixtures for the architecture checker's test-only exception."""

from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from architecture_sources import over_limit_sources, test_only_sources


class TestReachability(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write("crates/demo/Cargo.toml", '[package]\nname = "demo"\nversion = "0.1.0"\n')

    def write(self, path: str, body: str) -> Path:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body)
        return target.resolve()

    def test_cfg_test_ancestry_and_integration_helpers_are_exempt(self) -> None:
        self.write("crates/demo/src/lib.rs", "mod production;\n#[cfg(test)]\nmod tests;\n")
        production = self.write("crates/demo/src/production.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        test = self.write("crates/demo/src/tests.rs", "mod child;\nfn a() {}\nfn b() {}\n")
        child = self.write("crates/demo/src/tests/child.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        self.write("crates/demo/tests/public.rs", '#[path = "support/fixture.rs"]\nmod fixture;\n')
        helper = self.write("crates/demo/tests/support/fixture.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        exempt = test_only_sources(self.root)
        self.assertTrue({test, child, helper} <= exempt)
        self.assertNotIn(production, exempt)
        violations = {path.resolve() for path, _ in over_limit_sources(self.root, 2)}
        self.assertIn(production, violations)
        self.assertTrue({test, child, helper}.isdisjoint(violations))

    def test_mixed_unqualified_unreachable_and_sql_remain_capped(self) -> None:
        self.write(
            "crates/demo/src/lib.rs",
            'mod tests;\n#[cfg(test)]\nmod only_test;\ninclude!("shared.rs");\n',
        )
        mixed = self.write("crates/demo/src/tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        only_test = self.write("crates/demo/src/only_test.rs", 'include!("shared.rs");\nfn a() {}\nfn b() {}\n')
        shared = self.write("crates/demo/src/shared.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        unreachable = self.write("crates/demo/src/unreachable_tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        sql = self.write("crates/demo/migrations/test_fixture.sql", "SELECT 1;\nSELECT 2;\nSELECT 3;\n")
        exempt = test_only_sources(self.root)
        self.assertIn(only_test, exempt)
        self.assertTrue({mixed, shared, unreachable}.isdisjoint(exempt))
        violations = {path.resolve() for path, _ in over_limit_sources(self.root, 2)}
        self.assertTrue({mixed, shared, unreachable, sql} <= violations)
        self.assertNotIn(only_test, violations)

    def test_production_path_alias_overrides_test_only_name(self) -> None:
        self.write(
            "crates/demo/src/lib.rs",
            '#[cfg(test)]\nmod tests;\n#[path = "tests.rs"]\nmod production_alias;\n',
        )
        named_test = self.write("crates/demo/src/tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        self.assertNotIn(named_test, test_only_sources(self.root))
        self.assertIn(named_test, {path.resolve() for path, _ in over_limit_sources(self.root, 2)})

    def test_unresolved_include_disables_exemptions(self) -> None:
        self.write("crates/demo/src/lib.rs", '#[cfg(test)]\nmod tests;\ninclude!(concat!("x", ".rs"));\n')
        named_test = self.write("crates/demo/src/tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        self.assertNotIn(named_test, test_only_sources(self.root))

    def test_ambiguous_inline_module_is_not_mistaken_for_test_only(self) -> None:
        self.write(
            "crates/demo/src/lib.rs",
            '#[cfg(test)]\nmod tests;\nmod nested {\n    mod tests;\n}\n',
        )
        named_test = self.write("crates/demo/src/tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        self.write("crates/demo/src/nested/tests.rs", "fn a() {}\nfn b() {}\nfn c() {}\n")
        self.assertNotIn(named_test, test_only_sources(self.root))


if __name__ == "__main__":
    unittest.main()
