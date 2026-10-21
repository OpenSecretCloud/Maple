#!/usr/bin/env python3
"""Check the exact PEM bundles used by the enclave before and after its CA update."""

import hashlib
import re
import ssl
import subprocess
import sys
from pathlib import Path


EMSIGN_C1 = "125609AA301DA0A249B97A8239CB6A34216F44DCAC9F3954B14292F2E8C8608F"
EMSIGN_G3 = "86A1ECBA089C4A8D3BBE2734C612BA341D813E043CF9E8A862CD5C57A36BBE6B"
EMSIGN_C3 = "BC4D809B15189D78DB3E1D8CF4F9726A795DA1643CA5F1358E1DDB0EDC0D7EB3"
COMODO_AAA = "D7A7A0FB5D7E2731D771E9484EBCDEF71D5F0C3E0A2948782BC83EE0EA699EF4"
COMODO_ECC = "1793927A0614549789ADCE2F8F34F7F0B66D0F3AE3A3B84D21EC15DBBA4FADC7"


def fingerprints(openssl: str, bundle: Path) -> set[str]:
    # OpenSSL's PKCS#7 conversion understands both CERTIFICATE and TRUSTED
    # CERTIFICATE entries in the Nixpkgs bundle. Re-parse its resulting PEMs
    # rather than searching source comments or relying on an unbundled proxy.
    pkcs7 = subprocess.run(
        [openssl, "crl2pkcs7", "-nocrl", "-certfile", str(bundle)],
        check=True,
        capture_output=True,
    ).stdout
    pem = subprocess.run(
        [openssl, "pkcs7", "-print_certs"],
        input=pkcs7,
        check=True,
        capture_output=True,
    ).stdout
    certificates = re.findall(
        rb"-----BEGIN CERTIFICATE-----\s+.*?-----END CERTIFICATE-----",
        pem,
        re.DOTALL,
    )
    if len(certificates) < 100:
        raise AssertionError(f"parsed only {len(certificates)} certificates from {bundle}")
    return {
        hashlib.sha256(ssl.PEM_cert_to_DER_cert(certificate.decode("ascii"))).hexdigest().upper()
        for certificate in certificates
    }


def main() -> None:
    if len(sys.argv) != 7:
        raise SystemExit(
            "usage: test_enclave_ca_bundle.py OPENSSL OLD_BUNDLE OLD_TLS_BUNDLE "
            "AAA_CERT NEW_BUNDLE NEW_TLS_BUNDLE"
        )
    openssl, old_file, old_tls_file, aaa_file, new_file, new_tls_file = sys.argv[1:]
    old = fingerprints(openssl, Path(old_file))
    old_tls = fingerprints(openssl, Path(old_tls_file))
    new = fingerprints(openssl, Path(new_file))
    new_tls = fingerprints(openssl, Path(new_tls_file))

    removed_roots = {EMSIGN_C1, EMSIGN_C3, EMSIGN_G3}
    assert old - new == removed_roots, "full CA bundle changed beyond the three emSign roots"
    assert not new - old, "full CA bundle unexpectedly added roots"
    assert old_tls - new_tls == removed_roots, "TLS CA bundle changed beyond the three emSign roots"
    assert not new_tls - old_tls, "TLS CA bundle unexpectedly added roots"

    # The compromised keys also have a Sectigo/Comodo AAA cross-sign. That
    # anchor remains email-trusted, but not TLS-trusted, in both generated NSS
    # bundles. Check both purposes against the exact ca-bundle.crt that the
    # EIF entrypoint loads; this update does not change its S/MIME trust.
    assert COMODO_AAA in old and COMODO_AAA in new, "AAA is missing from the full CA bundle"
    assert COMODO_AAA not in old_tls and COMODO_AAA not in new_tls, "AAA entered the TLS bundle"
    for bundle in (old_file, new_file):
        rejected = subprocess.run(
            [openssl, "verify", "-purpose", "sslserver", "-CAfile", bundle, aaa_file],
            capture_output=True,
        )
        assert rejected.returncode != 0 and b"certificate rejected" in rejected.stderr, (
            f"AAA server trust was not rejected by {bundle}"
        )
        for purpose in ("smimesign", "smimeencrypt"):
            accepted = subprocess.run(
                [openssl, "verify", "-purpose", purpose, "-CAfile", bundle, aaa_file],
                capture_output=True,
            )
            assert accepted.returncode == 0, f"AAA {purpose} trust changed in {bundle}"

    assert COMODO_ECC in new_tls, "unrelated Comodo ECC root was removed"
    print(f"old TLS roots: {len(old_tls)}; enclave TLS roots: {len(new_tls)}; three emSign roots removed")


if __name__ == "__main__":
    main()
