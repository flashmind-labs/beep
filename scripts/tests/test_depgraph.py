import importlib.machinery
import importlib.util
import unittest
from pathlib import Path

path = Path(__file__).resolve().parents[1] / "depgraph"
spec = importlib.util.spec_from_loader("depgraph", importlib.machinery.SourceFileLoader("depgraph", str(path)))
depgraph = importlib.util.module_from_spec(spec)
spec.loader.exec_module(depgraph)


class WithoutCfgTest(unittest.TestCase):
    def test_hides_only_the_test_item(self):
        text = (
            '#[cfg(test)]\n#[path = "t.rs"]\nmod tests;\n'
            'use a::b;\n'
            '#[cfg(test)]\nuse x::{y, z};\n'
            '#[cfg(all(test, unix))]\nfn t() { let s = "}"; /* } */ let c = \'}\'; }\n'
            '#[cfg(test)]\nmod tests {\n    fn a() { sleep(1); }\n}\n'
            'fn real() { sleep(2); }\n'
        )
        out = depgraph.without_cfg_test(text)
        self.assertIn("use a::b;", out)
        self.assertIn("fn real() { sleep(2); }", out)
        self.assertNotIn("sleep(1)", out)
        self.assertNotIn("x::", out)
        self.assertNotIn("fn t", out)
        self.assertEqual(out.count("\n"), text.count("\n"))

    def test_keeps_non_test_cfg(self):
        text = '#[cfg(not(test))]\nfn a() {}\n#[cfg(unix)]\nfn b() {}\n'
        self.assertEqual(depgraph.without_cfg_test(text), text)

    def blanked(self, text):
        out = depgraph.without_cfg_test(text)
        self.assertEqual(out.count("\n"), text.count("\n"))
        return out

    def test_field_variant_and_arm_end_at_their_comma(self):
        out = self.blanked("struct S { #[cfg(test)] a: u8, b: X }\nimpl Y {}\n")
        self.assertIn("b: X }", out)
        self.assertIn("impl Y {}", out)
        self.assertNotIn("a: u8", out)
        out = self.blanked("enum E { #[cfg(test)] A(u8), B }\nfn z() {}\n")
        self.assertIn("B }", out)
        self.assertNotIn("A(", out)
        out = self.blanked("match x {\n #[cfg(test)] 1 => foo(),\n _ => bar(),\n}\n")
        self.assertIn("_ => bar()", out)
        self.assertNotIn("foo", out)

    def test_last_field_without_comma_stops_at_the_closer(self):
        out = self.blanked("struct S { b: X, #[cfg(test)] a: u8 }\nimpl Y {}\n")
        self.assertIn("b: X,", out)
        self.assertIn("}\nimpl Y {}", out)
        self.assertNotIn("a: u8", out)

    def test_only_a_positive_test_requirement_blanks(self):
        for pred in ("not(test)", "all(not(test), unix)", 'all(feature = "test-utils", unix)', "any(test, unix)"):
            text = f"#[cfg({pred})]\nfn a() {{}}\n"
            self.assertEqual(depgraph.without_cfg_test(text), text, pred)

    def test_const_fn_ends_at_its_body(self):
        out = self.blanked("#[cfg(test)]\nconst fn a() -> u8 { 1 }\nfn keep() {}\n")
        self.assertIn("fn keep() {}", out)
        self.assertNotIn("const fn", out)

    def test_markers_in_comments_and_strings_do_not_trigger(self):
        text = '// #[cfg(test)]\nfn a() { let s = "#[cfg(test)]"; }\n/* #[cfg(test)] */\nfn b() {}\n'
        self.assertEqual(depgraph.without_cfg_test(text), text)


if __name__ == "__main__":
    unittest.main()
