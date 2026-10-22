#!/usr/bin/env python3
"""Regenerate public fixtures using independent reference implementations.

Requires argon2-cffi (reference libargon2) and libsodium >=1.0.12. Nothing in
this file is a production secret. Example:
  PYTHONPATH=/tmp/maple-key-vector-deps python3 generate-vector.py /path/libsodium
The generated vector commits the exact versions and canonical bytes.
"""

import base64
import ctypes
import hashlib
import importlib.metadata
import json
from pathlib import Path
import struct
import sys
import unicodedata

from argon2.low_level import Type, hash_secret_raw

sodium = ctypes.CDLL(sys.argv[1])
sodium.sodium_init()
sodium.sodium_version_string.restype = ctypes.c_char_p
encrypt = sodium.crypto_aead_xchacha20poly1305_ietf_encrypt
encrypt.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulonglong),
                   ctypes.c_void_p, ctypes.c_ulonglong, ctypes.c_void_p,
                   ctypes.c_ulonglong, ctypes.c_void_p, ctypes.c_void_p,
                   ctypes.c_void_p]
encrypt.restype = ctypes.c_int

password = "Maple slot reference 🗝 e\u0301 2026!"
normalized = unicodedata.normalize("NFC", password).encode()
db_salt = bytes(range(160, 176))
salt = bytes(range(32))
nonce = bytes(range(32, 56))
dek = bytes(range(64, 96))
params = dict(m_kib=65536, t=3, p=4)
kek = hash_secret_raw(normalized, salt, params["t"], params["m_kib"],
                      params["p"], 32, Type.ID, version=19)

def text(value):
    encoded = value.encode()
    return struct.pack("<H", len(encoded)) + encoded

aad = (text("maple-store-keys/1") + db_salt + text("password") + text("argon2id")
       + struct.pack("<IIII", 19, params["m_kib"], params["t"], params["p"]) + salt)
ciphertext = ctypes.create_string_buffer(48)
length = ctypes.c_ulonglong()
assert encrypt(ciphertext, ctypes.byref(length), dek, len(dek), aad,
               len(aad), None, nonce, kek) == 0
assert length.value == 48
wrapped = ciphertext.raw
check = hashlib.sha256(aad + nonce + wrapped).digest()[:16]

def b64(value):
    return base64.urlsafe_b64encode(value).decode().rstrip("=")

vector = dict(
    provenance={"argon2-cffi": importlib.metadata.version("argon2-cffi"),
                "libsodium": sodium.sodium_version_string().decode(),
                "algorithms": "reference libargon2 Argon2id v1.3, libsodium XChaCha20-Poly1305"},
    password=password, normalized_password=normalized.decode(), dek_hex=dek.hex(),
    kek_hex=kek.hex(), slot_bytes_hex=aad.hex(), check_hex=check.hex(),
    key_file=dict(format="maple-store-keys/1", db_salt=b64(db_salt), slots=[dict(
        type="password", kdf=dict(alg="argon2id", v=19, **params, salt=b64(salt)),
        nonce=b64(nonce), wrapped=b64(wrapped), check=b64(check))]))
Path(__file__).with_name("slot-vector.json").write_text(
    json.dumps(vector, indent=2, ensure_ascii=False) + "\n")
