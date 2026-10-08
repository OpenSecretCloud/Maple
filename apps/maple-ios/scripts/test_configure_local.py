import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("configure_local", Path(__file__).with_name("configure-local.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ConfigureLocalTests(unittest.TestCase):
    def test_only_public_values_are_written_without_shell_evaluation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.env"
            source.write_text(
                "VITE_OPEN_SECRET_API_URL=http://127.0.0.1:31000\n"
                "VITE_MAPLE_BILLING_API_URL='http://localhost:36000'\n"
                "VITE_OS_FLAGS_BASE_URL=http://[::1]:41000\n"
                "PRIVATE_CANARY=must-not-copy\n"
                "IGNORED_COMMAND=$(touch should-not-exist)\n"
            )
            target = root / "generated.xcconfig"
            module.configure(source, "11111111-1111-4111-8111-111111111111", "http://127.0.0.1:35000", target)
            text = target.read_text()
            self.assertIn("MAPLE_API_URL = http:/$()/127.0.0.1:31000", text)
            self.assertIn("MAPLE_PROFILE = local", text)
            self.assertNotIn("must-not-copy", text)
            self.assertNotIn("IGNORED_COMMAND", text)
            self.assertEqual(target.stat().st_mode & 0o777, 0o600)

    def test_non_loopback_and_credential_bearing_urls_are_rejected(self):
        for value in [
            "https://example.com", "http://example.com:31000", "http://127.0.0.1",
            "http://127.0.0.1:0", "http://user:secret@127.0.0.1:31000",
            "http://127.0.0.1:31000/private", "http://127.0.0.1:31000?token=secret",
            "http://127.0.0.1:31000#fragment", "http://127.0.0.1:31000\nINJECT=1",
        ]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.loopback_origin(value)

    def test_missing_and_duplicate_settings_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "input.env"
            for text in [
                "VITE_OPEN_SECRET_API_URL=http://127.0.0.1:31000\n",
                "VITE_OPEN_SECRET_API_URL=http://127.0.0.1:31000\n" * 2,
            ]:
                source.write_text(text)
                with self.assertRaises(ValueError):
                    module.read_public_values(source)


if __name__ == "__main__":
    unittest.main()
