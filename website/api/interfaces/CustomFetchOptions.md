[**@opensecret/react**](../README.md)

***

# Interface: CustomFetchOptions

## Properties

### apiKey?

> `optional` **apiKey?**: `string`

Optional API key to use instead of a JWT token.

***

### apiUrl?

> `optional` **apiUrl?**: `string`

API URL used for attestation; required outside OpenSecretProvider.

***

### pcrConfig?

> `optional` **pcrConfig?**: [`PcrConfig`](../type-aliases/PcrConfig.md)

PCR0 trust policy enforced before non-loopback session key exchange.
