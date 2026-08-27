import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeDerUtf8String,
  verifyGenericFulcioOids
} from "./update-trusted-enclave-releases.mjs";

function encodeDerUtf8String(value) {
  const bytes = Buffer.from(value, "utf8");
  if (bytes.length < 0x80) {
    return Buffer.concat([Buffer.from([0x0c, bytes.length]), bytes]);
  }

  const lengthBytes = [];
  let length = bytes.length;
  while (length > 0) {
    lengthBytes.unshift(length & 0xff);
    length = Math.floor(length / 256);
  }
  return Buffer.concat([Buffer.from([0x0c, 0x80 | lengthBytes.length, ...lengthBytes]), bytes]);
}

function signerOid(oid, value) {
  return {
    oid: { id: oid.split(".").map(Number) },
    value: encodeDerUtf8String(value)
  };
}

test("decodes canonical short and long DER UTF8String values", () => {
  assert.equal(decodeDerUtf8String(encodeDerUtf8String("github-hosted")), "github-hosted");
  const longValue = "a".repeat(256);
  assert.equal(decodeDerUtf8String(encodeDerUtf8String(longValue)), longValue);
});

test("rejects malformed DER UTF8String values", () => {
  for (const malformed of [
    Buffer.from([0x16, 0x01, 0x61]),
    Buffer.from([0x0c, 0x80, 0x00, 0x00]),
    Buffer.from([0x0c, 0x81, 0x01, 0x61]),
    Buffer.from([0x0c, 0x82, 0x00, 0x80, ...Buffer.alloc(0x80)]),
    Buffer.from([0x0c, 0x02, 0x61]),
    Buffer.from([0x0c, 0x01, 0x61, 0x62]),
    Buffer.from([0x0c, 0x01, 0xff])
  ]) {
    assert.throws(() => decodeDerUtf8String(malformed));
  }
});

test("requires every generic Fulcio claim to match exactly", () => {
  const expected = {
    "1.3.6.1.4.1.57264.1.9": "workflow identity",
    "1.3.6.1.4.1.57264.1.21": "run invocation"
  };
  const signer = {
    identity: {
      oids: Object.entries(expected).map(([oid, value]) => signerOid(oid, value))
    }
  };

  assert.doesNotThrow(() => verifyGenericFulcioOids(signer, expected));
  assert.throws(
    () =>
      verifyGenericFulcioOids(
        {
          identity: {
            oids: [
              signerOid("1.3.6.1.4.1.57264.1.9", "workflow identity"),
              signerOid("1.3.6.1.4.1.57264.1.9", "workflow identity"),
              signerOid("1.3.6.1.4.1.57264.1.21", "run invocation")
            ]
          }
        },
        expected
      ),
    /duplicate OID/
  );
  assert.throws(
    () =>
      verifyGenericFulcioOids(
        {
          identity: {
            oids: [signerOid("1.3.6.1.4.1.57264.1.9", "workflow identity")]
          }
        },
        expected
      ),
    /missing OID/
  );
  assert.throws(
    () =>
      verifyGenericFulcioOids(
        {
          identity: {
            oids: [
              signerOid("1.3.6.1.4.1.57264.1.9", "wrong identity"),
              signerOid("1.3.6.1.4.1.57264.1.21", "run invocation")
            ]
          }
        },
        expected
      ),
    /unexpected OID/
  );
});
