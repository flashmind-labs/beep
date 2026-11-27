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


if __name__ == "__main__":
    unittest.main()
