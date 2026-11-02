"""Deploy verifies what it installs and never overwrites the console's config."""
from pathlib import Path
import tempfile
import unittest

import deploy

TL3 = '''[status]
listen = "192.168.1.107:8033"

[console]
listen = "192.168.1.107:8034"
assets = "/home/rob/.local/share/tl3/console"
user = "tl3_console"
password_file = "/home/rob/.config/tl3/secrets/console-postgres-password"
'''


class Artifacts(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        (self.directory / "bin").mkdir()
        (self.directory / "bin/tl3-console").write_bytes(b"binary")
        (self.directory / "assets").mkdir()
        (self.directory / "assets/index.html").write_text("<!doctype html>")
        (self.directory / "SHA256SUMS").write_text(deploy.checksums(self.directory))

    def test_checksums_cover_every_file(self):
        recorded = (self.directory / "SHA256SUMS").read_text()
        self.assertIn("  bin/tl3-console\n", recorded)
        self.assertIn("  assets/index.html\n", recorded)
        deploy.verify(self.directory)

    def test_a_changed_or_added_file_fails_verification(self):
        (self.directory / "assets/index.html").write_text("tampered")
        with self.assertRaises(RuntimeError):
            deploy.verify(self.directory)
        (self.directory / "assets/index.html").write_text("<!doctype html>")
        (self.directory / "assets/extra.js").write_text("")
        with self.assertRaises(RuntimeError):
            deploy.verify(self.directory)


class Config(unittest.TestCase):
    def test_config_starts_from_tl3s_console_section(self):
        home = Path("/home/rob")
        with tempfile.TemporaryDirectory() as directory:
            tl3 = Path(directory, "tl3.toml")
            tl3.write_text(TL3)
            text = deploy.console_config(home, tl3)
        self.assertIn('listen = "192.168.1.107:8034"\n', text)
        self.assertIn('user = "tl3_console"\n', text)
        self.assertIn('password_file = "/home/rob/.config/tl3/secrets/console-postgres-password"\n', text)
        self.assertIn('assets = "/home/rob/.local/share/tl3-console/current/assets"\n', text)
        self.assertIn(f'tl3_config = "{tl3}"\n', text)

    def test_a_tl3_config_without_a_console_login_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            tl3 = Path(directory, "tl3.toml")
            tl3.write_text('[console]\nlisten = "192.168.1.107:8034"\n')
            with self.assertRaises(RuntimeError):
                deploy.console_config(Path("/home/rob"), tl3)

    def test_write_once_keeps_an_existing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "tl3-console/console.toml")
            self.assertTrue(deploy.write_once(path, "first\n"))
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertFalse(deploy.write_once(path, "second\n"))
            self.assertEqual(path.read_text(), "first\n")


class Unit(unittest.TestCase):
    def test_unit_runs_the_current_release_with_the_console_config(self):
        directives = [line for line in deploy.unit_text().splitlines() if not line.startswith("#")]
        self.assertIn("ExecStart=%h/.local/share/tl3-console/current/bin/tl3-console "
                      "--config %h/.config/tl3-console/console.toml", directives)
        # Not TL3's wrapper, and no TL3 credential or config of its own.
        for word in ("deploy.py", "tl3.toml", "LoadCredential"):
            self.assertFalse(any(word in line for line in directives), word)


if __name__ == "__main__":
    unittest.main()
