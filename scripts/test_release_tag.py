import unittest
from pathlib import Path
import importlib.util

spec = importlib.util.spec_from_file_location("validate", Path(__file__).with_name("validate-release-tag.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class TagTests(unittest.TestCase):
    def test_valid(self):
        for tag in ("v1.0.0", "v1.0.0-alpha.1", "v1.0.0-beta.2", "v1.0.0-rc.1"):
            self.assertTrue(module.valid(tag), tag)

    def test_invalid(self):
        for tag in ("v01.0.0", "v1.0", "v1.0.0-alpha.01", "v1.0.0-foo/bar", "v1.0.0-"):
            self.assertFalse(module.valid(tag), tag)

if __name__ == "__main__":
    unittest.main()
