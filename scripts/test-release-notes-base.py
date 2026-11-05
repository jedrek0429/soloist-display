#!/usr/bin/env python3
"""Regression tests for prerelease and stable changelog baselines."""
import importlib.util
from pathlib import Path
import unittest

path = Path(__file__).with_name("release-notes-base.py")
spec = importlib.util.spec_from_file_location("release_notes_base", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
pick = module.pick


class PreviousReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tags = [
            "v1.0.0-alpha.9", "v1.0.0", "v1.1.0-alpha.1",
            "v1.1.0-alpha.2", "v1.1.0-alpha.3", "v1.1.0-alpha.4",
            "v1.1.0-beta.1", "v1.1.0-beta.2",
            "invalid", "v1.1.0-rc.1", "v2.0.0",
        ]

    def test_first_alpha_uses_previous_stable(self):
        self.assertEqual(pick("v1.1.0-alpha.1", self.tags), "v1.0.0")

    def test_alpha_uses_previous_alpha_not_old_alpha(self):
        self.assertEqual(pick("v1.1.0-alpha.4", self.tags), "v1.1.0-alpha.3")

    def test_first_beta_uses_last_alpha(self):
        self.assertEqual(pick("v1.1.0-beta.1", self.tags), "v1.1.0-alpha.4")

    def test_later_beta_uses_previous_beta(self):
        self.assertEqual(pick("v1.1.0-beta.2", self.tags), "v1.1.0-beta.1")

    def test_stable_uses_previous_stable_only(self):
        self.assertEqual(pick("v1.1.0", self.tags), "v1.0.0")

    def test_no_predecessor(self):
        self.assertEqual(pick("v0.1.0-alpha.1", []), "")

    def test_ignores_future_and_current_tags(self):
        self.assertEqual(pick("v1.0.0-alpha.9", self.tags), "")

    def test_no_previous_channel_fallback_when_same_channel_exists(self):
        self.assertEqual(pick("v1.1.0-beta.3", self.tags), "v1.1.0-beta.2")

    def test_no_following_stable_fallback(self):
        self.assertEqual(pick("v1.0.0-alpha.1", ["v1.0.0", "v2.0.0"]), "")

    def test_invalid_current_tag(self):
        with self.assertRaises(ValueError):
            pick("v1.1.0-rc.1", self.tags)


if __name__ == "__main__":
    unittest.main()
