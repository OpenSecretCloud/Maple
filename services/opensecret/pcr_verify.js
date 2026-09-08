#!/usr/bin/env node

const fs = require('fs');
const crypto = require('crypto');

/**
 * Verifies a PCR0 signature
 * @param {string} pcr0 - The PCR0 value
 * @param {string} signatureBase64 - Base64-encoded signature
 * @param {Object} publicKey - Public key object
 * @returns {boolean} True if valid, false otherwise
 */
function verifyPcr0Signature(pcr0, signatureBase64, publicKey) {
  try {
    // Create the verifier
    const verifier = crypto.createVerify('SHA384');
    verifier.update(pcr0);
    
    // Decode the base64 signature
    const signature = Buffer.from(signatureBase64, 'base64');
    
    // Verify using the P1363 format
    return verifier.verify({
      key: publicKey,
      dsaEncoding: 'ieee-p1363'
    }, signature);
  } catch (error) {
    console.error(`Error verifying signature: ${error.message}`);
    return false;
  }
}

/**
 * Extract public key from different formats
 * @param {string} publicKeyString - Base64-encoded public key
 * @returns {Object} Public key object
 */
function extractPublicKey(publicKeyString) {
  try {
    return crypto.createPublicKey({
      key: Buffer.from(publicKeyString, 'base64'),
      format: 'der',
      type: 'spki'
    });
  } catch (error) {
    // Try to parse as base64-encoded PEM
    try {
      const decodedKey = Buffer.from(publicKeyString, 'base64').toString('utf8');
      
      // If it's a PEM format, extract the key
      if (decodedKey.includes('-----BEGIN PUBLIC KEY-----')) {
        return crypto.createPublicKey(decodedKey);
      } else if (decodedKey.includes('-----BEGIN PRIVATE KEY-----')) {
        console.error('ERROR: You provided a private key instead of a public key!');
        console.error('Please use SIGNING_PUBLIC_KEY, not SIGNING_PRIVATE_KEY');
        process.exit(1);
      }
    } catch (error) {
      console.error("Error processing public key:", error.message);
      process.exit(1);
    }
  }

  console.error("Failed to parse public key");
  process.exit(1);
}

/**
 * Main verification function
 */
function main() {
  // Get environment from command line
  const env = process.argv[2];
  
  if (!env || (env !== 'dev' && env !== 'prod')) {
    console.log("Usage: pcr_verify.js <env>");
    console.log("  env: Environment to verify (dev or prod)");
    process.exit(1);
  }
  
  // Determine which history file to use
  const historyFile = env === 'dev' ? './pcrDevHistory.json' : './pcrProdHistory.json';
  
  // Check if the file exists
  if (!fs.existsSync(historyFile)) {
    console.error(`Error: History file ${historyFile} does not exist`);
    process.exit(1);
  }
  
  // Get public key from environment variable
  const publicKeyBase64 = process.env.SIGNING_PUBLIC_KEY;
  if (!publicKeyBase64) {
    console.error("Error: SIGNING_PUBLIC_KEY environment variable is not set");
    console.log("Set it with: export SIGNING_PUBLIC_KEY='your-base64-public-key'");
    process.exit(1);
  }
  
  // Import the public key
  const publicKey = extractPublicKey(publicKeyBase64);
  
  // Read and parse the history file
  let history;
  try {
    const historyData = fs.readFileSync(historyFile, 'utf8');
    history = JSON.parse(historyData);
  } catch (error) {
    console.error(`Error reading history file: ${error.message}`);
    process.exit(1);
  }
  
  // Validate that history is an array
  if (!Array.isArray(history)) {
    console.error(`Error: ${historyFile} does not contain a valid array`);
    process.exit(1);
  }
  
  console.log(`\n🔍 Verifying ${history.length} PCR entries in ${historyFile}...\n`);
  
  // Track verification results
  let validCount = 0;
  let invalidCount = 0;
  
  // Verify each entry
  for (let i = 0; i < history.length; i++) {
    const entry = history[i];
    
    // Check if entry has required fields
    if (!entry.PCR0 || !entry.signature) {
      console.error(`❌ Entry ${i}: Missing required fields (PCR0 or signature)`);
      invalidCount++;
      continue;
    }
    
    // Verify the signature (only for PCR0)
    const isValid = verifyPcr0Signature(entry.PCR0, entry.signature, publicKey);
    
    if (isValid) {
      validCount++;
      const dateStr = entry.timestamp 
        ? new Date(entry.timestamp * 1000).toLocaleString() 
        : 'unknown date';
        
      console.log(`✅ Entry ${i}: Valid (${dateStr})`);
      console.log(`   PCR0: ${entry.PCR0.substring(0, 40)}...`);
    } else {
      invalidCount++;
      console.log(`❌ Entry ${i}: INVALID signature`);
      console.log(`   PCR0: ${entry.PCR0.substring(0, 40)}...`);
    }
  }
  
  // Print summary
  console.log(`\n📊 Verification Summary:`);
  console.log(`   Total entries: ${history.length}`);
  console.log(`   Valid signatures: ${validCount}`);
  console.log(`   Invalid signatures: ${invalidCount}`);
  
  // Exit with appropriate code
  if (invalidCount > 0) {
    console.log(`\n⚠️  Warning: ${invalidCount} entries have invalid signatures!`);
    process.exit(1);
  } else {
    console.log(`\n✅ All ${validCount} signatures are valid!`);
    process.exit(0);
  }
}

// Run the main function if this script is executed directly
if (require.main === module) {
  main();
}

// Export functions for potential module usage
module.exports = {
  verifyPcr0Signature,
  extractPublicKey
}; 