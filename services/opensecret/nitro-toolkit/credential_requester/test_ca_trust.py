"""Offline trust-store check for the deployable credential requester image."""

import hashlib
from pathlib import Path
import re
import ssl
import unittest

import botocore.httpsession
import botocore.session
import certifi


# Reviewed certifi 2026.10.19 bundle: 120 roots after removal of the
# compromised eMudhra emSign ECC Root CA - G3 from the previous 121-root bundle.
EXPECTED_CERTIFI_VERSION = "2026.10.19"
EXPECTED_BUNDLE_SHA256 = "50f79f98dac192b20dbf73f35c56f0b0c557ab267ff5199d1117cc1180b70785"
EMUDHRA_G3_SHA256 = "86a1ecba089c4a8d3bbe2734c612ba341d813e043cf9e8a862cd5c57a36bbe6b"


class CredentialRequesterCATrustTests(unittest.TestCase):
    def test_installed_bundle_and_botocore_default_trust(self):
        bundle = Path(certifi.where())
        data = bundle.read_bytes()
        certs = re.findall(
            rb"-----BEGIN CERTIFICATE-----\s+.*?-----END CERTIFICATE-----",
            data,
            re.DOTALL,
        )
        fingerprints = {
            hashlib.sha256(ssl.PEM_cert_to_DER_cert(cert.decode("ascii"))).hexdigest()
            for cert in certs
        }

        self.assertEqual(certifi.__version__, EXPECTED_CERTIFI_VERSION)
        self.assertEqual(hashlib.sha256(data).hexdigest(), EXPECTED_BUNDLE_SHA256)
        self.assertEqual((len(certs), len(fingerprints)), (120, 120))
        self.assertNotIn(EMUDHRA_G3_SHA256, fingerprints)

        session = botocore.session.get_session()
        self.assertIsNone(session.get_config_variable("ca_bundle"))
        client = session.create_client(
            "secretsmanager",
            region_name="us-east-2",
            aws_access_key_id="synthetic-access-key",
            aws_secret_access_key="synthetic-secret-key",
            aws_session_token="synthetic-session-token",
        )
        try:
            # Check the effective TLS verifier selected by this pinned Botocore
            # client, without sending a request or reading AWS credentials.
            verify = client._endpoint.http_session._verify
            self.assertEqual(botocore.httpsession.get_cert_path(verify), str(bundle))
        finally:
            client.close()


if __name__ == "__main__":
    unittest.main()
