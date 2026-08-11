---
title: Remote Attestation
sidebar_position: 8
---

# Remote Attestation

Remote attestation is a key security feature of OpenSecret that allows your application to verify the authenticity and security posture of the server running your code. This guide explains how remote attestation works in OpenSecret and how to configure it in your applications.

## Overview

Remote attestation provides cryptographic proof that:

1. Your code is running in a genuine AWS Nitro enclave
2. The enclave has not been tampered with
3. The code running inside the enclave matches what you expect
4. The communication channel is secure

OpenSecret implements a comprehensive attestation system that combines both:
- Local attestation verification using known PCR values
- Remote attestation using signed PCR history from a trusted source

## How OpenSecret's Attestation Works

When your application communicates with OpenSecret:

1. **Request Attestation**: Your client requests an attestation document with a random nonce
2. **Verify Document**: The SDK validates the attestation document's signature and certificate chain
3. **Verify PCR0**: The SDK verifies the PCR0 value against known good values and remote attestation
4. **Key Exchange**: A secure session key is established for encrypted communications
5. **Ongoing Protection**: The session key is stored in session storage for subsequent requests

## Understanding PCR Values

PCR (Platform Configuration Register) values are cryptographic measurements of the enclave's state:

- **PCR0**: Hash of the enclave's kernel and initial RAM disk (most critical)
- **PCR1**: Hash of the boot command line parameters
- **PCR2**: Hash of user-provided modules

OpenSecret primarily validates PCR0, which is the most critical measurement as it represents the core components of the enclave.

## PCR0 Validation Process

OpenSecret uses an environment-scoped approach to PCR0 validation. The trust environment defaults
to production and must be set explicitly to development for a hosted development enclave:

1. **Local Validation**: The SDK checks only the selected environment's built-in and custom PCR0
   values. Static production roots are not checked by a development policy, and static development
   roots are not checked by a production policy.

2. **Signed History**: If local validation doesn't find a match, the SDK fetches only the selected
   environment's pinned-key signed history:
   - Production: `https://raw.githubusercontent.com/OpenSecretCloud/opensecret/master/pcrProdHistory.json`
   - Development: `https://raw.githubusercontent.com/OpenSecretCloud/opensecret/master/pcrDevHistory.json`

3. **Signature Verification**: The SDK verifies matching history entries with OpenSecret's pinned
   public key. It never falls back to the other environment's history.

Environment separation for signed history is currently enforced by selecting one pinned history
URL. History signatures authenticate the PCR0 value with a shared verification key; they do not
cryptographically encode the environment. Separate keys or an environment-bearing signed payload
would be required for cryptographic domain separation between the two history files.

## Configuration Options

OpenSecret provides several options for configuring remote attestation to meet your security requirements:

### Basic PCR Configuration

```tsx
import { OpenSecretProvider } from "@opensecret/react";

function App() {
  return (
    <OpenSecretProvider 
      apiUrl="https://api.opensecret.cloud"
      clientId="your-project-uuid"
      pcrConfig={{
        // Optional: production is the default trust environment.
        environment: "production",
        // Additional roots are scoped to the selected environment.
        pcr0Values: [
          "your-trusted-pcr0-value-1",
          "your-trusted-pcr0-value-2"
        ]
      }}
    >
      <YourApp />
    </OpenSecretProvider>
  );
}
```

For a hosted development enclave, select development explicitly and use `pcr0DevValues` for any
additional development roots:

```tsx
<OpenSecretProvider
  apiUrl="https://your-development-enclave.example"
  clientId="your-project-uuid"
  pcrConfig={{
    environment: "development",
    pcr0DevValues: ["your-dev-pcr0-value"]
  }}
>
  <YourApp />
</OpenSecretProvider>
```

### Disabling Remote Attestation

In some cases, you might want to rely only on local PCR0 validation:

```tsx
import { OpenSecretProvider } from "@opensecret/react";

function App() {
  return (
    <OpenSecretProvider 
      apiUrl="https://api.opensecret.cloud"
      clientId="your-project-uuid"
      pcrConfig={{
        environment: "production",
        // Disable remote attestation - only use local PCR0 values
        remoteAttestation: false,
        // Your trusted PCR0 values
        pcr0Values: ["your-trusted-pcr0-value"]
      }}
    >
      <YourApp />
    </OpenSecretProvider>
  );
}
```

### Custom Remote Attestation URLs

You can specify custom URLs for signed-history verification. Only the URL for the selected
environment is requested:

```tsx
import { OpenSecretProvider } from "@opensecret/react";

function App() {
  return (
    <OpenSecretProvider 
      apiUrl="https://api.opensecret.cloud"
      clientId="your-project-uuid"
      pcrConfig={{
        environment: "production",
        // Custom URLs for remote attestation
        remoteAttestationUrls: {
          prod: "https://your-custom-url/pcrProdHistory.json",
          dev: "https://your-custom-url/pcrDevHistory.json"
        }
      }}
    >
      <YourApp />
    </OpenSecretProvider>
  );
}
```

## Accessing Attestation Information

You can manually request and verify attestation information:

```tsx
import { useState } from "react";
import { useOpenSecret } from "@opensecret/react";

function AttestationInfo() {
  const os = useOpenSecret();
  const [attestation, setAttestation] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  
  async function verifyAttestation() {
    setLoading(true);
    setError("");
    try {
      // Get and verify the attestation document
      const attestationDoc = await os.getAttestationDocument();
      setAttestation(attestationDoc);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Attestation verification failed");
      setAttestation(null);
    } finally {
      setLoading(false);
    }
  }
  
  return (
    <div>
      <h3>Enclave Attestation Verification</h3>
      
      <button onClick={verifyAttestation} disabled={loading}>
        {loading ? "Verifying..." : "Verify Enclave Security"}
      </button>
      
      {error && <div className="error">{error}</div>}
      
      {attestation && (
        <div>
          <h4>Attestation Verified</h4>
          
          <div>
            <strong>Instance ID:</strong> {attestation.moduleId}
          </div>
          
          <div>
            <strong>PCR0 Validation:</strong> {" "}
            {attestation.validatedPcr0Hash ? (
              attestation.validatedPcr0Hash.isMatch ? (
                <span className="valid">✅ {attestation.validatedPcr0Hash.text}</span>
              ) : (
                <span className="invalid">❌ {attestation.validatedPcr0Hash.text}</span>
              )
            ) : (
              "PCR0 not validated"
            )}
            {attestation.validatedPcr0Hash?.verifiedAt && (
              <div>
                <strong>Verified at:</strong> {attestation.validatedPcr0Hash.verifiedAt}
              </div>
            )}
          </div>
          
          <div>
            <strong>Timestamp:</strong> {attestation.timestamp}
          </div>
          
          <div>
            <strong>Nonce:</strong> {attestation.nonce}
          </div>
          
          <div>
            <strong>PCRs:</strong>
            <ul>
              {attestation.pcrs.map(pcr => (
                <li key={pcr.id}>
                  PCR{pcr.id}: {pcr.value}
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </div>
  );
}
```

## Understanding the Attestation Document

The attestation document returned by `getAttestationDocument()` includes:

- **moduleId**: The ID of the Nitro enclave module
- **publicKey**: The enclave's public key (base64 encoded)
- **timestamp**: When the attestation document was generated
- **digest**: The hash algorithm used (SHA384)
- **pcrs**: Array of PCR values with their IDs
- **certificates**: The certificate chain used for validation
- **userData**: Optional user data included in the attestation
- **nonce**: The nonce that was used to prevent replay attacks
- **cert0hash**: Hash of the AWS root certificate
- **validatedPcr0Hash**: Results of PCR0 validation

## How PCR0 Validation Works in Detail

OpenSecret's PCR0 validation process follows these steps:

1. **Local Static Validation**:
   - Defaults to the production trust environment
   - Checks only the selected environment's hardcoded PCR0 values
   - Checks only custom PCR0 values for that environment (`pcr0Values` for production or
     `pcr0DevValues` for development)

2. **Remote Attestation** (if enabled):
   - Fetches signed PCR history only for the selected environment
   - Verifies the authenticity of each entry using OpenSecret's public key
   - Checks if the PCR0 value matches any entry in the history
   - Validates the signature on the matching entry
   - Does not fall back to the other environment

3. **Result Reporting**:
   - Returns a validation result indicating whether the PCR0 value is trusted
   - Includes information about whether the match was found via local validation or remote attestation
   - For remote attestation matches, includes when the PCR0 value was verified

## Security Considerations

### Production Best Practices

For production deployments:

1. **Keep the production default or select it explicitly**: Never configure a production client
   with `environment: "development"`.

2. **Use explicit PCR values when appropriate**: Additional values are additive to the selected
   environment's built-in roots.

3. **Keep signed-history lookup enabled**: The signed GitHub history lets approved backend PCR0
   values rotate without requiring every client to update immediately.

4. **Verify attestation before sensitive operations**: Always verify attestation before sending
   sensitive data to the enclave.

5. **Rotate sensitive data**: If you discover that a PCR0 value is no longer trusted, rotate any
   sensitive data that was sent to that enclave.

### Obtaining Trusted PCR0 Values

For a production deployment:

1. **Request official PCR0 values** from OpenSecret during onboarding
2. **Verify these values** through multiple channels if possible
3. **Pin these values** in your application configuration
4. **Monitor for updates** to the enclave image that might change PCR0 values

## Debugging Attestation

When developing with OpenSecret, you might need to debug attestation issues:

```tsx
import { useOpenSecret } from "@opensecret/react";

function AttestationDebugger() {
  const os = useOpenSecret();
  
  async function debugAttestation() {
    try {
      const attestationDoc = await os.getAttestationDocument();
      
      console.log("Attestation document:", attestationDoc);
      console.log("PCR values:");
      attestationDoc.pcrs.forEach(pcr => {
        console.log(`PCR${pcr.id}: ${pcr.value}`);
      });
      
      console.log("PCR0 validation result:", attestationDoc.validatedPcr0Hash);
      
      if (attestationDoc.validatedPcr0Hash?.isMatch) {
        console.log("✅ PCR0, validated! You can use this PCR0 value in your configuration.");
      } else {
        console.log("❌ PCR0, not validated. Do not use this PCR0 value in production!");
      }
    } catch (error) {
      console.error("Attestation error:", error);
    }
  }
  
  return (
    <button onClick={debugAttestation}>
      Debug Attestation
    </button>
  );
}
```

## Local Development

When running in development mode with a localhost API (`http://localhost:3000` or similar), OpenSecret automatically uses a fake attestation document to allow local development without a real enclave.

This allows you to develop your application locally while still maintaining the same code structure that works with real attestation in production.

## Next Steps

- [Data Encryption](./data-encryption) - Learn how to encrypt sensitive data
- [Cryptographic Operations](./cryptographic-operations) - Explore OpenSecret's cryptographic features
- [AI Integration](./ai-integration) - Understand how AI is integrated with security guarantees
