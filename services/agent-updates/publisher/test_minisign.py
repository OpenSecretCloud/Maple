"""Verify the shared public wire fixture with the pinned real minisign CLI."""

import json
import hashlib
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import publisher


FIXTURES = Path(__file__).with_name("fixtures")


@unittest.skipUnless(shutil.which("minisign"), "run in the pinned root CI shell for cryptographic tests")
class MinisignTests(unittest.TestCase):
    def setUp(self):
        self.data = (FIXTURES / "signed-envelope.json").read_bytes()
        self.key = FIXTURES / "public-test-key.pub"

    def test_shared_client_fixture_verifies_prehashed_with_exact_comment(self):
        manifest = publisher.minisign_verify(self.data, self.key)
        self.assertEqual(manifest["version"], "0.0.2")
        envelope = json.loads(self.data)
        self.assertEqual(envelope["manifest"].encode(), (FIXTURES / "manifest.json").read_bytes())
        self.assertEqual(envelope["signature"].encode(), (FIXTURES / "manifest.minisig").read_bytes())

    def test_tampered_signed_bytes_and_comment_are_rejected(self):
        envelope = json.loads(self.data)
        envelope["manifest"] = envelope["manifest"].replace("A test update.", "A fake update.")
        with self.assertRaisesRegex(publisher.PublishError, "verification failed"):
            publisher.minisign_verify(publisher.encode_json(envelope), self.key)
        envelope = json.loads(self.data)
        envelope["signature"] = envelope["signature"].replace("channel=stable", "channel=preview")
        with self.assertRaisesRegex(publisher.PublishError, "comment"):
            publisher.minisign_verify(publisher.encode_json(envelope), self.key)

    def test_real_temporary_key_reservation_signing_verification_and_retry(self):
        class ReservationStore:
            def __init__(self):
                self.objects = {}

            def put(self, key, data, *, condition, content_type):
                self.objects[key] = data

            def get(self, key, maximum=publisher.MAX_JSON):
                return (self.objects[key], '"fixture-etag"') if key in self.objects else None

        # These disposable, offline test keys never leave the private tempdir.
        # Production key generation is an owner operation outside this tool.
        with tempfile.TemporaryDirectory(prefix="maple-agent-minisign-test-") as temporary:
            bundle = Path(temporary)
            key = bundle / "public-key.pub"
            secret = bundle / "test-only-secret.key"
            result = subprocess.run(["minisign", "-G", "-W", "-p", str(key), "-s", str(secret)], stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, "offline temporary test key generation failed")
            (bundle / "manifest.json").write_bytes((FIXTURES / "manifest.json").read_bytes())
            payloads = bundle / "payloads"
            payloads.mkdir()
            for name in ("Maple-Agent.app.tar.gz", "Maple-Agent.dmg"):
                (payloads / name).write_bytes(b"signed payload fixture bytes")
            metadata = bundle / "metadata"
            metadata.mkdir()
            inventory = {}
            for name in sorted({"provenance.json"} | publisher.MACOS_METADATA_NAMES):
                data = ("offline signing test sidecar: " + name + "\n").encode()
                (metadata / name).write_bytes(data)
                inventory[name] = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
            (bundle / "metadata-inventory.json").write_bytes(publisher.encode_json(inventory))
            store = ReservationStore()
            publisher.reserve(bundle, store)
            self.assertIn("stable/releases/0.0.2/publication-manifest.json", store.objects)
            publisher.sign(bundle, secret, key)
            original = (bundle / "envelope.json").read_bytes()
            self.assertEqual(publisher.minisign_verify(original, key)["version"], "0.0.2")
            publisher.sign(bundle, secret, key)
            self.assertEqual((bundle / "envelope.json").read_bytes(), original)
            # A repeated signing attempt is deterministic: the exact trusted
            # comment and untrusted comment contain no implicit timestamp.
            (bundle / "envelope.json").unlink()
            publisher.sign(bundle, secret, key)
            self.assertEqual((bundle / "envelope.json").read_bytes(), original)
            envelope = json.loads(original)
            envelope["manifest"] = envelope["manifest"].replace("A test update.", "A fake update.")
            with self.assertRaisesRegex(publisher.PublishError, "verification failed"):
                publisher.minisign_verify(publisher.encode_json(envelope), key)


if __name__ == "__main__":
    unittest.main()
