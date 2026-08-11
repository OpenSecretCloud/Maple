[**@opensecret/react**](../README.md)

***

# Type Alias: PcrConfig

> **PcrConfig** = `object`

Configuration options for PCR validation

## Properties

### pcr0DevValues?

> `optional` **pcr0DevValues?**: `string`[]

Additional trusted PCR0 values for development environments.
The SDK's built-in development trust roots always remain trusted.

***

### pcr0Values?

> `optional` **pcr0Values?**: `string`[]

Additional trusted PCR0 values for production environments.
The SDK's built-in production trust roots always remain trusted.

***

### remoteAttestation?

> `optional` **remoteAttestation?**: `boolean`

Whether to consult pinned-key signed PCR history after local trust roots miss
(defaults to true). This does not enable or disable Nitro attestation verification.

***

### remoteAttestationUrls?

> `optional` **remoteAttestationUrls?**: `object`

Custom URLs for pinned-key signed PCR history

#### dev?

> `optional` **dev?**: `string`

URL for development PCR history

#### prod?

> `optional` **prod?**: `string`

URL for production PCR history
