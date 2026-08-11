[**@opensecret/react**](../README.md)

***

# Type Alias: PcrConfig

> **PcrConfig** = `object`

Configuration options for PCR validation

## Properties

### environment?

> `optional` **environment?**: [`PcrEnvironment`](PcrEnvironment.md)

OpenSecret deployment environment to trust (defaults to production).
Only this environment's embedded roots, additional roots, and signed
history are considered during session establishment.

***

### pcr0DevValues?

> `optional` **pcr0DevValues?**: `string`[]

Additional trusted PCR0 values for development environments.
These and the SDK's built-in development roots are considered only when
`environment` is `"development"`.

***

### pcr0Values?

> `optional` **pcr0Values?**: `string`[]

Additional trusted PCR0 values for production environments.
These and the SDK's built-in production roots are considered only when
`environment` is `"production"`.

***

### remoteAttestation?

> `optional` **remoteAttestation?**: `boolean`

Whether to consult pinned-key signed PCR history after local trust roots miss
(defaults to true). This does not enable or disable Nitro attestation verification.

***

### remoteAttestationUrls?

> `optional` **remoteAttestationUrls?**: `object`

Custom URLs for pinned-key signed PCR history. Only the selected environment is fetched.

#### dev?

> `optional` **dev?**: `string`

URL for development PCR history

#### prod?

> `optional` **prod?**: `string`

URL for production PCR history
