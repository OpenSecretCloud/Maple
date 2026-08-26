import { expect, test, beforeEach } from "bun:test";
import {
  fetchLogin,
  fetchPrivateKey,
  fetchPrivateKeyBytes,
  fetchPublicKey,
  signMessage,
  encryptData,
  decryptData,
  KeyOptions
} from "../../api";
import { schnorr } from "@noble/curves/secp256k1";

const TEST_EMAIL = process.env.VITE_TEST_EMAIL;
const TEST_PASSWORD = process.env.VITE_TEST_PASSWORD;
const TEST_CLIENT_ID = process.env.VITE_TEST_CLIENT_ID;

if (!TEST_EMAIL || !TEST_PASSWORD || !TEST_CLIENT_ID) {
  throw new Error("Test credentials must be set in .env.local");
}

async function setupTestUser() {
  try {
    // Try to login
    const { access_token } = await fetchLogin(TEST_EMAIL!, TEST_PASSWORD!, TEST_CLIENT_ID!);
    window.localStorage.setItem("access_token", access_token);

    // Add a small delay to ensure tokens are properly set
    await new Promise((resolve) => setTimeout(resolve, 100));

    // Verify tokens were set correctly
    const storedToken = window.localStorage.getItem("access_token");
    if (!storedToken) {
      throw new Error("Failed to set access token");
    }
  } catch (error: any) {
    throw new Error("Failed to set up test user: " + error.message);
  }
}

// Clean up before each test
beforeEach(async () => {
  window.localStorage.clear();
});

// BIP-85 test constants
const BIP85_STANDARD_PATH = "m/83696968'/39'/0'/12'/0'";
const BIP85_ALTERNATIVE_PATH = "m/83696968'/39'/0'/12'/1'";
const BIP32_PATH = "m/44'/0'/0'/0/0";

test("BIP-85 derive child mnemonic", async () => {
  await setupTestUser();

  // Get master mnemonic
  const masterMnemonicResponse = await fetchPrivateKey();
  expect(masterMnemonicResponse.mnemonic).toBeDefined();
  expect(typeof masterMnemonicResponse.mnemonic).toBe("string");

  // The mnemonic should be 12 words (or more, depending on server configuration)
  const wordCount = masterMnemonicResponse.mnemonic.split(" ").length;
  expect(wordCount).toBeGreaterThanOrEqual(12);

  // Get child mnemonic using BIP-85
  const childMnemonicResponse = await fetchPrivateKey({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });
  expect(childMnemonicResponse.mnemonic).toBeDefined();
  expect(typeof childMnemonicResponse.mnemonic).toBe("string");
  expect(childMnemonicResponse.mnemonic.split(" ").length).toBe(12);

  // Child mnemonic should be different from master
  expect(childMnemonicResponse.mnemonic).not.toBe(masterMnemonicResponse.mnemonic);

  // Different BIP-85 paths should generate different mnemonics
  const alternativeChildMnemonicResponse = await fetchPrivateKey({
    seed_phrase_derivation_path: BIP85_ALTERNATIVE_PATH
  });
  expect(alternativeChildMnemonicResponse.mnemonic).not.toBe(childMnemonicResponse.mnemonic);

  // This section was removed since we no longer support string parameters
});

test("BIP-85 private key bytes", async () => {
  await setupTestUser();

  // Get master private key bytes
  const masterKeyResponse = await fetchPrivateKeyBytes();
  expect(masterKeyResponse.private_key).toBeDefined();
  expect(masterKeyResponse.private_key).toMatch(/^[0-9a-f]{64}$/i);

  // Get BIP-85 derived private key bytes (master key of child seed)
  const bip85KeyResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });
  expect(bip85KeyResponse.private_key).toBeDefined();
  expect(bip85KeyResponse.private_key).toMatch(/^[0-9a-f]{64}$/i);

  // BIP-85 derived key should be different from master
  expect(bip85KeyResponse.private_key).not.toBe(masterKeyResponse.private_key);

  // Different BIP-85 paths should generate different keys
  const alternativeBip85KeyResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_ALTERNATIVE_PATH
  });
  expect(alternativeBip85KeyResponse.private_key).not.toBe(bip85KeyResponse.private_key);

  // Same BIP-85 path should consistently generate the same key
  const repeatBip85KeyResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });
  expect(repeatBip85KeyResponse.private_key).toBe(bip85KeyResponse.private_key);
});

test("Combined BIP-85 and BIP-32 derivation", async () => {
  await setupTestUser();

  // Get BIP-85 derived private key bytes
  const bip85OnlyResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });

  // Get BIP-32 derived private key bytes
  const bip32OnlyResponse = await fetchPrivateKeyBytes({
    private_key_derivation_path: BIP32_PATH
  });

  // Get combined BIP-85 + BIP-32 derived private key bytes
  const combinedResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH,
    private_key_derivation_path: BIP32_PATH
  });

  // All keys should be different
  expect(bip85OnlyResponse.private_key).not.toBe(bip32OnlyResponse.private_key);
  expect(combinedResponse.private_key).not.toBe(bip85OnlyResponse.private_key);
  expect(combinedResponse.private_key).not.toBe(bip32OnlyResponse.private_key);

  // Same derivation paths should consistently generate the same key
  const repeatCombinedResponse = await fetchPrivateKeyBytes({
    seed_phrase_derivation_path: BIP85_STANDARD_PATH,
    private_key_derivation_path: BIP32_PATH
  });
  expect(repeatCombinedResponse.private_key).toBe(combinedResponse.private_key);
});

// This test was removed since we no longer support string parameters

test("BIP-85 public key derivation", async () => {
  await setupTestUser();

  // Get master public key
  const masterPubKey = await fetchPublicKey("schnorr");

  // Get BIP-85 derived public key
  const bip85PubKey = await fetchPublicKey("schnorr", {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });

  // Get BIP-32 derived public key
  const bip32PubKey = await fetchPublicKey("schnorr", {
    private_key_derivation_path: BIP32_PATH
  });

  // Get combined BIP-85 + BIP-32 derived public key
  const combinedPubKey = await fetchPublicKey("schnorr", {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH,
    private_key_derivation_path: BIP32_PATH
  });

  // All public keys should be different
  expect(masterPubKey.public_key).not.toBe(bip85PubKey.public_key);
  expect(bip85PubKey.public_key).not.toBe(bip32PubKey.public_key);
  expect(combinedPubKey.public_key).not.toBe(bip85PubKey.public_key);
  expect(combinedPubKey.public_key).not.toBe(bip32PubKey.public_key);

  // Test ECDSA algorithm too
  const bip85EcdsaPubKey = await fetchPublicKey("ecdsa", {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });
  expect(bip85EcdsaPubKey.public_key).toBeDefined();
  expect(bip85EcdsaPubKey.algorithm).toBe("ecdsa");
});

test("Sign messages with BIP-85 derived keys", async () => {
  await setupTestUser();

  const message = new TextEncoder().encode("Hello, BIP-85!");

  // Sign with master key
  const masterSignature = await signMessage(message, "schnorr");
  const masterPubKey = await fetchPublicKey("schnorr");

  // Sign with BIP-85 derived key
  const bip85Options: KeyOptions = {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  };
  const bip85Signature = await signMessage(message, "schnorr", bip85Options);
  const bip85PubKey = await fetchPublicKey("schnorr", bip85Options);

  // Sign with combined BIP-85 + BIP-32 derived key
  const combinedOptions: KeyOptions = {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH,
    private_key_derivation_path: BIP32_PATH
  };
  const combinedSignature = await signMessage(message, "schnorr", combinedOptions);
  const combinedPubKey = await fetchPublicKey("schnorr", combinedOptions);

  // All signatures should be different
  expect(masterSignature.signature).not.toBe(bip85Signature.signature);
  expect(bip85Signature.signature).not.toBe(combinedSignature.signature);
  expect(masterSignature.signature).not.toBe(combinedSignature.signature);

  // All signatures should verify against their corresponding public keys
  const isValidMaster = schnorr.verify(
    masterSignature.signature,
    masterSignature.message_hash,
    masterPubKey.public_key
  );
  expect(isValidMaster).toBe(true);

  const isValidBip85 = schnorr.verify(
    bip85Signature.signature,
    bip85Signature.message_hash,
    bip85PubKey.public_key
  );
  expect(isValidBip85).toBe(true);

  const isValidCombined = schnorr.verify(
    combinedSignature.signature,
    combinedSignature.message_hash,
    combinedPubKey.public_key
  );
  expect(isValidCombined).toBe(true);

  // Verify that a signature doesn't verify against a different key
  const isInvalidCross = schnorr.verify(
    bip85Signature.signature,
    bip85Signature.message_hash,
    masterPubKey.public_key
  );
  expect(isInvalidCross).toBe(false);
});

test("Encrypt and decrypt with BIP-85 derived keys", async () => {
  await setupTestUser();

  const testData = "This is a secret message for BIP-85 testing";

  // Test data array with different key derivation options
  const testCases = [
    { name: "Master key", options: undefined },
    { name: "BIP-32 only", options: { private_key_derivation_path: BIP32_PATH } },
    { name: "BIP-85 only", options: { seed_phrase_derivation_path: BIP85_STANDARD_PATH } },
    {
      name: "Combined BIP-85 + BIP-32",
      options: {
        seed_phrase_derivation_path: BIP85_STANDARD_PATH,
        private_key_derivation_path: BIP32_PATH
      }
    }
  ];

  // Run tests for each derivation option
  for (const testCase of testCases) {
    // Encrypt with the specified key
    const encryptResponse = await encryptData(testData, testCase.options);
    expect(encryptResponse.encrypted_data).toBeDefined();

    // Decrypt with the same key
    const decryptedData = await decryptData(encryptResponse.encrypted_data, testCase.options);
    expect(decryptedData).toBe(testData);

    // Mixing derivation paths should fail
    if (testCase.options) {
      try {
        // Try decrypting with a different derivation path
        const wrongOptions =
          testCase.name === "BIP-32 only"
            ? { seed_phrase_derivation_path: BIP85_STANDARD_PATH }
            : { private_key_derivation_path: BIP32_PATH };

        await decryptData(encryptResponse.encrypted_data, wrongOptions);
        throw new Error(`Should not decrypt ${testCase.name} with wrong derivation path`);
      } catch (error: any) {
        expect(error.message).toBe("Bad Request");
      }
    }
  }

  // Test that different BIP-85 paths produce different encryption results
  const bip85Encryption1 = await encryptData(testData, {
    seed_phrase_derivation_path: BIP85_STANDARD_PATH
  });

  const bip85Encryption2 = await encryptData(testData, {
    seed_phrase_derivation_path: BIP85_ALTERNATIVE_PATH
  });

  expect(bip85Encryption1.encrypted_data).not.toBe(bip85Encryption2.encrypted_data);

  // This section was removed since we no longer support string parameters
});

test("Error handling for invalid BIP-85 paths", async () => {
  await setupTestUser();

  // Test invalid BIP-85 paths
  const invalidPaths = [
    "m/83696968/39'/0'/12'/0'", // Missing hardened marker on first index
    "m/1'/39'/0'/12'/0'", // Wrong application number
    "m/83696968'/1'/0'/12'/0'", // Wrong application type
    "m/83696968'/39'/0'/5'/0'" // Invalid entropy (not 12, 18, or 24)
  ];

  for (const invalidPath of invalidPaths) {
    try {
      await fetchPrivateKey({
        seed_phrase_derivation_path: invalidPath
      });
      throw new Error(`Should not accept invalid BIP-85 path: ${invalidPath}`);
    } catch (error: any) {
      expect(error.message).toBe("Bad Request");
    }
  }
});
