import { expect, test, beforeEach } from "bun:test";
import { fetchLogin, signMessage, fetchPublicKey, fetchPrivateKeyBytes } from "../../api";
import { bytesToHex } from "../utils";
import { sha256 } from "@noble/hashes/sha256";
import { schnorr, secp256k1 } from "@noble/curves/secp256k1";

const TEST_EMAIL = process.env.VITE_TEST_EMAIL;
const TEST_PASSWORD = process.env.VITE_TEST_PASSWORD;
const TEST_CLIENT_ID = process.env.VITE_TEST_CLIENT_ID;

if (!TEST_EMAIL || !TEST_PASSWORD || !TEST_CLIENT_ID) {
  throw new Error("Test credentials must be set in .env.local");
}

async function tryEmailLogin() {
  try {
    // Try to login
    const loginResponse = await fetchLogin(TEST_EMAIL!, TEST_PASSWORD!, TEST_CLIENT_ID!);
    window.localStorage.setItem("access_token", loginResponse.access_token);
    window.localStorage.setItem("refresh_token", loginResponse.refresh_token);

    return loginResponse;
  } catch (error: any) {
    throw new Error("Failed to set up test user: " + error.message);
  }
}

// Clean up before each test
beforeEach(async () => {
  window.localStorage.clear();
});

// These helper functions are not needed - all tests will log in explicitly

test("Sign message with Schnorr returns valid signature", async () => {
  const { access_token, refresh_token } = await tryEmailLogin();
  window.localStorage.setItem("access_token", access_token);
  window.localStorage.setItem("refresh_token", refresh_token);

  // First get the public key
  const { public_key } = await fetchPublicKey("schnorr");
  expect(public_key).toBeDefined();
  expect(typeof public_key).toBe("string");
  expect(public_key).toMatch(/^[0-9a-f]{64}$/i); // 32 bytes = 64 hex chars

  // Then sign a message
  const message = new TextEncoder().encode("Hello, World!");
  const response = await signMessage(message, "schnorr");

  // Verify we got all fields
  expect(response.signature).toBeDefined();
  expect(response.message_hash).toBeDefined();

  // Verify types and formats
  expect(typeof response.signature).toBe("string");
  expect(typeof response.message_hash).toBe("string");

  // Verify hex format and lengths
  expect(response.signature).toMatch(/^[0-9a-f]{128}$/i); // 64 bytes = 128 hex chars
  expect(response.message_hash).toMatch(/^[0-9a-f]{64}$/i); // 32 bytes = 64 hex chars

  // Verify the hash matches our local hash
  const localHash = sha256(message);
  expect(bytesToHex(localHash)).toBe(response.message_hash.toLowerCase());

  // Verify the signature
  const isValid = schnorr.verify(response.signature, response.message_hash, public_key);
  expect(isValid).toBe(true);
});

test("Sign message with ECDSA returns valid signature", async () => {
  const { access_token, refresh_token } = await tryEmailLogin();
  window.localStorage.setItem("access_token", access_token);
  window.localStorage.setItem("refresh_token", refresh_token);

  // First get the public key
  const { public_key } = await fetchPublicKey("ecdsa");
  expect(public_key).toBeDefined();
  expect(typeof public_key).toBe("string");
  expect(public_key).toMatch(/^[0-9a-f]{66}$/i); // 33 bytes = 66 hex chars (compressed format)

  // Then sign a message
  const message = new TextEncoder().encode("Hello, World!");
  const response = await signMessage(message, "ecdsa");

  // Verify we got all fields
  expect(response.signature).toBeDefined();
  expect(response.message_hash).toBeDefined();

  // Verify types and formats
  expect(typeof response.signature).toBe("string");
  expect(typeof response.message_hash).toBe("string");

  // Verify hex format and lengths
  expect(response.signature).toMatch(/^[0-9a-f]{130,144}$/i); // Adjust range to include shorter valid signatures
  expect(response.message_hash).toMatch(/^[0-9a-f]{64}$/i); // 32 bytes = 64 hex chars

  // Verify the hash matches our local hash
  const localHash = sha256(message);
  expect(bytesToHex(localHash)).toBe(response.message_hash.toLowerCase());

  // Verify the signature using noble/curves
  const isValid = secp256k1.verify(response.signature, response.message_hash, public_key);
  expect(isValid).toBe(true);
});

test("Private key endpoints with derivation paths", async () => {
  // Login first to get authenticated
  const { access_token, refresh_token } = await tryEmailLogin();
  window.localStorage.setItem("access_token", access_token);
  window.localStorage.setItem("refresh_token", refresh_token);

  // Test getting private key bytes without derivation path
  const masterKeyResponse = await fetchPrivateKeyBytes();
  expect(masterKeyResponse.private_key).toBeDefined();
  expect(typeof masterKeyResponse.private_key).toBe("string");
  expect(masterKeyResponse.private_key.length).toBe(64);
  expect(masterKeyResponse.private_key).toMatch(/^[0-9a-f]{64}$/i);

  // Test getting private key bytes with valid derivation paths
  const validPaths = [
    "m/44'/0'/0'/0/0", // BIP44
    "m/84'/0'/0'/0/0", // BIP84
    "m/49'/0'/0'/0/0", // BIP49
    "m/44'/1'/0'/0/0", // BIP44 testnet
    "m/44'/60'/0'/0/0", // BIP44 Ethereum
    "44'/0'/0'/0/0", // Relative path
    "0/0", // Simple relative path
    "1/2", // Relative path with different indices
    "m", // Master key
    "" // Empty path (master key)
  ];

  for (const path of validPaths) {
    const keyOptions = path ? { private_key_derivation_path: path } : undefined;
    const response = await fetchPrivateKeyBytes(keyOptions);
    expect(response.private_key).toBeDefined();
    expect(typeof response.private_key).toBe("string");
    expect(response.private_key.length).toBe(64);
    expect(response.private_key).toMatch(/^[0-9a-f]{64}$/i);

    // Get corresponding public key
    const pubKeyResponse = await fetchPublicKey("schnorr", keyOptions);
    expect(pubKeyResponse.public_key).toBeDefined();

    // Sign and verify with derived key
    const message = new TextEncoder().encode("Test message");
    const signResponse = await signMessage(message, "schnorr", keyOptions);
    const isValid = schnorr.verify(
      signResponse.signature,
      signResponse.message_hash,
      pubKeyResponse.public_key
    );
    expect(isValid).toBe(true);

    // Add a small delay between iterations to avoid throttling
    await new Promise((resolve) => setTimeout(resolve, 200));
  }

  // Test invalid derivation path
  await expect(
    fetchPrivateKeyBytes({ private_key_derivation_path: "invalid/path" })
  ).rejects.toThrow("Bad Request");

  // Test signing with derivation path
  const path = { private_key_derivation_path: "m/44'/0'/0'/0/0" };
  const message = new TextEncoder().encode("Hello, World!");
  const signResponse = await signMessage(message, "schnorr", path);
  expect(signResponse.signature).toBeDefined();
  expect(signResponse.message_hash).toBeDefined();

  // Test getting public key with derivation path
  const pubKeyResponse = await fetchPublicKey("schnorr", path);
  expect(pubKeyResponse.public_key).toBeDefined();
  expect(pubKeyResponse.algorithm).toBe("schnorr");

  // Test ECDSA signing with derivation path
  const ecdsaSignResponse = await signMessage(message, "ecdsa", path);
  expect(ecdsaSignResponse.signature).toBeDefined();
  expect(ecdsaSignResponse.message_hash).toBeDefined();

  // Test getting ECDSA public key with derivation path
  const ecdsaPubKeyResponse = await fetchPublicKey("ecdsa", path);
  expect(ecdsaPubKeyResponse.public_key).toBeDefined();
  expect(ecdsaPubKeyResponse.algorithm).toBe("ecdsa");

  // Verify that different derivation paths produce different keys
  const path1Response = await fetchPrivateKeyBytes({
    private_key_derivation_path: "m/44'/0'/0'/0/0"
  });
  const path2Response = await fetchPrivateKeyBytes({
    private_key_derivation_path: "m/44'/0'/0'/0/1"
  });
  expect(path1Response.private_key).not.toBe(path2Response.private_key);

  // Test hardened derivation with different coin types
  const coinTypes = ["0'", "1'", "60'", "145'"];
  const hardenedKeys = await Promise.all(
    coinTypes.map((coin) =>
      fetchPrivateKeyBytes({
        private_key_derivation_path: `m/44'/${coin}/0'/0/0`
      })
    )
  );
  const uniqueKeys = new Set(hardenedKeys.map((k) => k.private_key));
  expect(uniqueKeys.size).toBe(coinTypes.length);

  // Test relative path edge cases
  const relativePathTests = [
    "0", // Single level
    "0/0/0", // Multiple levels
    "2147483647", // Max index
    "0'/1/2", // Mix of hardened and non-hardened
    "0h/1/2" // Alternative hardened notation
  ];

  for (const path of relativePathTests) {
    const keyOptions = { private_key_derivation_path: path };
    const response = await fetchPrivateKeyBytes(keyOptions);
    expect(response.private_key).toBeDefined();
    expect(response.private_key).toMatch(/^[0-9a-f]{64}$/i);

    // Verify corresponding public key works
    const pubKey = await fetchPublicKey("schnorr", keyOptions);
    const msg = new TextEncoder().encode("Test relative path");
    const sig = await signMessage(msg, "schnorr", keyOptions);
    const isValid = schnorr.verify(sig.signature, sig.message_hash, pubKey.public_key);
    expect(isValid).toBe(true);
  }
}, 30000); // Extend timeout to 30s

test("Sign message fails without authentication", async () => {
  window.localStorage.removeItem("access_token");
  window.localStorage.removeItem("refresh_token");

  const message = new TextEncoder().encode("Hello, World!");
  await expect(signMessage(message, "schnorr")).rejects.toThrow("No access token available");
});

test("Different messages produce different signatures and hashes", async () => {
  // Login first to get authenticated
  const { access_token, refresh_token } = await tryEmailLogin();
  window.localStorage.setItem("access_token", access_token);
  window.localStorage.setItem("refresh_token", refresh_token);

  // Get the public key once
  const { public_key } = await fetchPublicKey("schnorr");

  const message1 = new TextEncoder().encode("Hello, World!");
  const message2 = new TextEncoder().encode("Different message");

  const response1 = await signMessage(message1, "schnorr");
  const response2 = await signMessage(message2, "schnorr");

  // Different signatures and hashes
  expect(response1.signature).not.toBe(response2.signature);
  expect(response1.message_hash).not.toBe(response2.message_hash);

  // Verify both signatures
  const isValid1 = schnorr.verify(response1.signature, response1.message_hash, public_key);
  expect(isValid1).toBe(true);

  const isValid2 = schnorr.verify(response2.signature, response2.message_hash, public_key);
  expect(isValid2).toBe(true);
});

test("Public key remains constant", async () => {
  // Login first to get authenticated
  const { access_token, refresh_token } = await tryEmailLogin();
  window.localStorage.setItem("access_token", access_token);
  window.localStorage.setItem("refresh_token", refresh_token);

  const response1 = await fetchPublicKey("schnorr");
  const response2 = await fetchPublicKey("schnorr");

  expect(response1.public_key).toBe(response2.public_key);
});
