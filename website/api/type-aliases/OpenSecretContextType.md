[**@opensecret/react**](../README.md)

***

# Type Alias: OpenSecretContextType

> **OpenSecretContextType** = `object`

## Properties

### aiCustomFetch

> **aiCustomFetch**: (`input`, `init?`) => `Promise`\<`Response`\>

Custom fetch function for AI requests that handles encryption
and token refreshing.

Meant to be used with the OpenAI JS library

Example:
```tsx
const openai = new OpenAI({
  baseURL: `${os.apiUrl}/v1/`,
  dangerouslyAllowBrowser: true,
  apiKey: "the-api-key-doesnt-matter",
  defaultHeaders: {
    "Accept-Encoding": "identity"
  },
  fetch: os.aiCustomFetch
});
```

#### Parameters

##### input

`string` \| `URL` \| `Request`

##### init?

`RequestInit`

#### Returns

`Promise`\<`Response`\>

***

### apiKey?

> `optional` **apiKey?**: `string`

Optional API key for OpenAI endpoints.
When set, this will be used instead of JWT for /v1/* endpoints.

***

### apiUrl

> **apiUrl**: `string`

Returns the current OpenSecret enclave API URL being used

#### Returns

The current API URL

***

### auth

> **auth**: [`OpenSecretAuthState`](OpenSecretAuthState.md)

***

### authenticate

> **authenticate**: *typeof* `authenticate`

Authenticates an attestation document

***

### awsRootCertDer

> **awsRootCertDer**: *typeof* `AWS_ROOT_CERT_DER`

AWS root certificate in DER format

***

### batchDeleteConversations

> **batchDeleteConversations**: *typeof* [`batchDeleteConversations`](../functions/batchDeleteConversations.md)

Batch deletes multiple conversations by their IDs

#### Param

Array of conversation UUIDs to delete

#### Returns

A promise resolving to per-item deletion results

***

### batchUpdateConversationProject

> **batchUpdateConversationProject**: *typeof* [`batchUpdateConversationProject`](../functions/batchUpdateConversationProject.md)

Batch moves conversations into a project or clears project assignment.

***

### cancelResponse

> **cancelResponse**: (`responseId`) => `Promise`\<[`ResponsesCancelResponse`](ResponsesCancelResponse.md)\>

Cancels an in-progress response

#### Parameters

##### responseId

`string`

The UUID of the response to cancel

#### Returns

`Promise`\<[`ResponsesCancelResponse`](ResponsesCancelResponse.md)\>

A promise resolving to the cancelled response

***

### changePassword

> **changePassword**: *typeof* `api.changePassword`

***

### checkDocumentStatus

> **checkDocumentStatus**: (`taskId`) => `Promise`\<[`DocumentStatusResponse`](DocumentStatusResponse.md)\>

Checks the status of a document processing task

#### Parameters

##### taskId

`string`

The task ID returned from uploadDocument

#### Returns

`Promise`\<[`DocumentStatusResponse`](DocumentStatusResponse.md)\>

A promise resolving to the current status and optionally the processed document

#### Throws

If:
- The user is not authenticated
- The task ID is not found (404)
- The user doesn't have access to the task (403)

#### Description

This function checks the status of an async document processing task.
Status values include:
- "pending": Document is queued for processing
- "started": Document processing has begun
- "success": Processing completed successfully (document field will be populated)
- "failure": Processing failed (error field will contain details)

Example usage:
```typescript
const status = await context.checkDocumentStatus(taskId);
if (status.status === "success" && status.document) {
  console.log(status.document.text);
}
```

***

### clientId

> **clientId**: `string`

The client ID for this project/tenant.
A UUID that identifies which project/tenant this instance belongs to.

***

### confirmAccountDeletion

> **confirmAccountDeletion**: (`confirmationCode`, `plaintextSecret`) => `Promise`\<`void`\>

Confirms and completes the account deletion process

#### Parameters

##### confirmationCode

`string`

The confirmation code from the verification email

##### plaintextSecret

`string`

The plaintext secret that was hashed in the request step

#### Returns

`Promise`\<`void`\>

A promise resolving to void

#### Throws

If confirmation fails

This function:
1. Requires the user to be logged in (uses authenticatedApiCall)
2. Verifies both the confirmation code from email and the secret known only to the client
3. Permanently deletes the user account and all associated data
4. After successful deletion, the client should clear all local storage and tokens

***

### confirmPasswordReset

> **confirmPasswordReset**: (`email`, `alphanumericCode`, `plaintextSecret`, `newPassword`) => `Promise`\<`void`\>

#### Parameters

##### email

`string`

##### alphanumericCode

`string`

##### plaintextSecret

`string`

##### newPassword

`string`

#### Returns

`Promise`\<`void`\>

***

### createApiKey

> **createApiKey**: *typeof* [`createApiKey`](../functions/createApiKey.md)

Creates a new API key for the authenticated user

#### Param

A descriptive name for the API key

#### Returns

A promise resolving to the API key details with the key value (only shown once)

#### Throws

If the user is not authenticated or the request fails

IMPORTANT: The `key` field is only returned once during creation and cannot be retrieved again.
The SDK consumer should prompt users to save the key immediately.

***

### createConversation

> **createConversation**: *typeof* [`createConversation`](../functions/createConversation.md)

Creates a single conversation with optional metadata/project/pin state.

***

### createConversationProject

> **createConversationProject**: *typeof* [`createConversationProject`](../functions/createConversationProject.md)

Creates a new conversation project.

***

### createInstruction

> **createInstruction**: *typeof* `api.createInstruction`

Creates a new instruction

#### Param

The instruction creation parameters

#### Returns

A promise resolving to the created instruction

#### Throws

If the user is not authenticated or the request fails

#### Description

Creates a new user instruction (system prompt).
If is_default is set to true, all other instructions are automatically set to is_default: false.
The prompt_tokens field is automatically calculated.

***

### createResponse

> **createResponse**: *typeof* `api.createResponse`

Creates a new response with conversation support

#### Param

The request parameters for creating a response

#### Returns

A promise resolving to the created response or a stream

***

### decryptData

> **decryptData**: *typeof* `api.decryptData`

Decrypts data that was previously encrypted with the user's key

#### Param

Base64-encoded encrypted data string

#### Param

Optional key derivation options or legacy BIP32 derivation path string

#### Returns

A promise resolving to the decrypted string

#### Throws

If:
- The encrypted data is malformed
- The derivation paths are invalid
- Authentication fails
- Server-side decryption error occurs

This function supports multiple decryption approaches:

1. Decrypt with master key (no derivation parameters)

2. Decrypt with BIP-32 derived key
   - Derives a child key from the master seed using BIP-32

3. Decrypt with BIP-85 derived key
   - Derives a child mnemonic using BIP-85, then uses its master key

4. Decrypt with combined BIP-85 and BIP-32 derivation
   - First derives a child mnemonic via BIP-85
   - Then applies BIP-32 derivation to derive a key from that seed

IMPORTANT: You must use the exact same derivation options for decryption
that were used for encryption.

***

### del

> **del**: *typeof* `api.fetchDelete`

Deletes a key-value pair from the user's storage

#### Param

The unique identifier for the value to be deleted

#### Returns

A promise resolving when the deletion is complete

#### Throws

If the key cannot be deleted

- Calls the authenticated API endpoint to remove a specific key
- Requires an active authentication session
- Throws an error if the deletion fails (including for non-existent keys)
- Propagates any server-side errors directly

***

### delAll

> **delAll**: *typeof* `api.fetchDeleteAllKV`

Deletes all key-value pairs from the user's storage

#### Returns

A promise resolving when the deletion is complete

#### Throws

If the deletion fails

***

### deleteApiKey

> **deleteApiKey**: *typeof* [`deleteApiKey`](../functions/deleteApiKey.md)

Deletes an API key by its name

#### Param

The name of the API key to delete

#### Returns

A promise that resolves when the key is deleted

#### Throws

If the user is not authenticated or the API key is not found

Permanently deletes an API key. This action cannot be undone.
Any requests using the deleted key will immediately fail with 401 Unauthorized.
Names are unique per user, so this uniquely identifies the key to delete.

***

### deleteConversation

> **deleteConversation**: *typeof* [`deleteConversation`](../functions/deleteConversation.md)

Deletes a single conversation.

***

### deleteConversationProject

> **deleteConversationProject**: *typeof* [`deleteConversationProject`](../functions/deleteConversationProject.md)

Deletes a conversation project.

***

### deleteConversations

> **deleteConversations**: *typeof* [`deleteConversations`](../functions/deleteConversations.md)

Deletes all conversations

#### Returns

A promise resolving to deletion confirmation

***

### deleteInstruction

> **deleteInstruction**: *typeof* `api.deleteInstruction`

Deletes an instruction permanently

#### Param

The UUID of the instruction to delete

#### Returns

A promise resolving to deletion confirmation

#### Throws

If the instruction is not found or user doesn't have access

#### Description

Permanently deletes an instruction. This action cannot be undone.

***

### deleteResponse

> **deleteResponse**: (`responseId`) => `Promise`\<[`ResponsesDeleteResponse`](ResponsesDeleteResponse.md)\>

Deletes a response permanently

#### Parameters

##### responseId

`string`

The UUID of the response to delete

#### Returns

`Promise`\<[`ResponsesDeleteResponse`](ResponsesDeleteResponse.md)\>

A promise resolving to deletion confirmation

***

### encryptData

> **encryptData**: *typeof* `api.encryptData`

Encrypts arbitrary string data using the user's private key

#### Param

String content to be encrypted

#### Param

Optional key derivation options or legacy BIP32 derivation path string

#### Returns

A promise resolving to the encrypted data response

#### Throws

If:
- The derivation paths are invalid
- Authentication fails
- Server-side encryption error occurs

This function supports multiple encryption approaches:

1. Encrypt with master key (no derivation parameters)

2. Encrypt with BIP-32 derived key
   - Derives a child key from the master seed using BIP-32
   - Example: "m/44\'/0\'/0\'/0/0"

3. Encrypt with BIP-85 derived key
   - Derives a child mnemonic using BIP-85, then uses its master key
   - Example: { seed_phrase_derivation_path: "m/83696968\'/39\'/0\'/12\'/0\'" }

4. Encrypt with combined BIP-85 and BIP-32 derivation
   - First derives a child mnemonic via BIP-85
   - Then applies BIP-32 derivation to derive a key from that seed
   - Example: {
       seed_phrase_derivation_path: "m/83696968\'/39\'/0\'/12\'/0\'",
       private_key_derivation_path: "m/44\'/0\'/0\'/0/0"
     }

Technical details:
- Encrypts data with AES-256-GCM
- A random nonce is generated for each encryption operation (included in the result)
- The encrypted_data format includes the nonce and is base64-encoded

***

### expectedRootCertHash

> **expectedRootCertHash**: *typeof* `EXPECTED_ROOT_CERT_HASH`

Expected hash of the AWS root certificate

***

### fetchModels

> **fetchModels**: () => `Promise`\<[`Model`](../interfaces/Model.md)[]\>

Fetches available AI models from the OpenAI-compatible API

#### Returns

`Promise`\<[`Model`](../interfaces/Model.md)[]\>

A promise resolving to an array of Model objects

#### Throws

If:
- The user is not authenticated
- The request fails

- Returns a list of available AI models from the configured OpenAI-compatible API
- Response is encrypted and automatically decrypted
- Guest users will receive a 401 Unauthorized error
- Requires an active authentication session

***

### fetchResponse

> **fetchResponse**: (`responseId`) => `Promise`\<[`ResponsesRetrieveResponse`](ResponsesRetrieveResponse.md)\>

Retrieves a single response by ID

#### Parameters

##### responseId

`string`

The UUID of the response to retrieve

#### Returns

`Promise`\<[`ResponsesRetrieveResponse`](ResponsesRetrieveResponse.md)\>

A promise resolving to the response details

***

### fetchResponsesList

> **fetchResponsesList**: (`params?`) => `Promise`\<[`ResponsesListResponse`](ResponsesListResponse.md)\>

Lists user's responses with pagination

#### Parameters

##### params?

[`ResponsesListParams`](ResponsesListParams.md)

Optional parameters for pagination and filtering

#### Returns

`Promise`\<[`ResponsesListResponse`](ResponsesListResponse.md)\>

A promise resolving to a paginated list of responses

#### Throws

If:
- The user is not authenticated
- The request fails
- Invalid pagination parameters

#### Description

This function fetches a paginated list of the user's responses.
In list view, the usage and output fields are always null for performance reasons.

Query Parameters:
- limit: Number of results per page (1-100, default: 20)
- after: UUID cursor for forward pagination
- before: UUID cursor for backward pagination
- order: Sort order (currently not implemented, reserved for future use)

Pagination Examples:
```typescript
// First page
const responses = await context.fetchResponsesList({ limit: 20 });

// Next page
const nextPage = await context.fetchResponsesList({
  limit: 20,
  after: responses.last_id
});

// Previous page
const prevPage = await context.fetchResponsesList({
  limit: 20,
  before: responses.first_id
});
```

***

### generateThirdPartyToken

> **generateThirdPartyToken**: (`audience?`) => `Promise`\<`ThirdPartyTokenResponse`\>

Generates a JWT token for use with third-party services

#### Parameters

##### audience?

`string`

Optional audience value for the target service

#### Returns

`Promise`\<`ThirdPartyTokenResponse`\>

A promise resolving to the token response

#### Throws

If:
- The user is not authenticated
- The audience value is invalid (if provided)

- Generates a signed JWT token for use with third-party services
- If audience is provided, it can be a valid URL or another accepted audience string
- If audience is omitted, a token with no audience restriction will be generated
- Requires an active authentication session
- Token can be used to authenticate with the specified service

***

### get

> **get**: *typeof* `api.fetchGet`

Retrieves a value from key-value storage

#### Param

The unique identifier for the stored value

#### Returns

A promise resolving to the stored value

#### Throws

If the key cannot be retrieved

- Calls the authenticated API endpoint to fetch a value
- Returns undefined if the key does not exist
- Requires an active authentication session
- Logs any retrieval errors

***

### getAttestation

> **getAttestation**: *typeof* `getAttestation`

Gets an attested session after enforcing the effective PCR0 trust policy

***

### getAttestationDocument

> **getAttestationDocument**: () => `Promise`\<[`ParsedAttestationView`](ParsedAttestationView.md)\>

Gets and verifies an attestation document from the enclave

#### Returns

`Promise`\<[`ParsedAttestationView`](ParsedAttestationView.md)\>

A promise resolving to the parsed attestation document

#### Throws

If attestation fails or is invalid

This is a convenience function that:
1. Fetches the attestation document with a random nonce
2. Authenticates the document
3. Parses it for viewing

***

### getConversation

> **getConversation**: *typeof* [`getConversation`](../functions/getConversation.md)

Retrieves a single conversation.

***

### getConversationItem

> **getConversationItem**: *typeof* [`getConversationItem`](../functions/getConversationItem.md)

Retrieves a single conversation item.

***

### getConversationProject

> **getConversationProject**: *typeof* [`getConversationProject`](../functions/getConversationProject.md)

Retrieves a single conversation project.

***

### getInstruction

> **getInstruction**: *typeof* `api.getInstruction`

Retrieves a single instruction by ID

#### Param

The UUID of the instruction to retrieve

#### Returns

A promise resolving to the instruction details

#### Throws

If the instruction is not found or user doesn't have access

***

### getPrivateKey

> **getPrivateKey**: *typeof* `api.fetchPrivateKey`

Retrieves the user's private key mnemonic phrase

#### Param

Optional key derivation options

#### Returns

A promise resolving to the private key response

#### Throws

If the private key cannot be retrieved

This function supports two modes:

1. Master mnemonic (no parameters)
   - Returns the user's master 12-word BIP39 mnemonic

2. BIP-85 derived mnemonic
   - Derives a child mnemonic using BIP-85
   - Requires seed_phrase_derivation_path in options
   - Example: "m/83696968'/39'/0'/12'/0'"

***

### getPrivateKeyBytes

> **getPrivateKeyBytes**: *typeof* `api.fetchPrivateKeyBytes`

Retrieves the private key bytes for the given derivation options

#### Param

Optional key derivation options or legacy BIP32 derivation path string

#### Returns

A promise resolving to the private key bytes response

#### Throws

If:
- The private key bytes cannot be retrieved
- The derivation paths are invalid

This function supports multiple derivation approaches:

1. Master key only (no parameters)
   - Returns the master private key bytes

2. BIP-32 derivation only
   - Uses a single derivation path to derive a child key from the master seed
   - Supports both absolute and relative paths with hardened derivation:
     - Absolute path: "m/44'/0'/0'/0/0"
     - Relative path: "0'/0'/0'/0/0"
     - Hardened notation: "44'" or "44h"
   - Common paths:
     - BIP44 (Legacy): m/44'/0'/0'/0/0
     - BIP49 (SegWit): m/49'/0'/0'/0/0
     - BIP84 (Native SegWit): m/84'/0'/0'/0/0
     - BIP86 (Taproot): m/86'/0'/0'/0/0

3. BIP-85 derivation only
   - Derives a child mnemonic from the master seed using BIP-85
   - Then returns the master private key of that derived seed
   - Example path: "m/83696968'/39'/0'/12'/0'"

4. Combined BIP-85 and BIP-32 derivation
   - First derives a child mnemonic via BIP-85
   - Then applies BIP-32 derivation to that derived seed

***

### getPublicKey

> **getPublicKey**: *typeof* `api.fetchPublicKey`

Retrieves the user's public key for the specified algorithm

#### Param

The signing algorithm ('schnorr' or 'ecdsa')

#### Param

Optional key derivation options or legacy BIP32 derivation path string

#### Returns

A promise resolving to the public key response

#### Throws

If the public key cannot be retrieved

The derivation paths determine which key is used to generate the public key:

1. Master key (no derivation parameters)
   - Returns the public key corresponding to the master private key

2. BIP-32 derived key
   - Returns the public key for a derived child key

3. BIP-85 derived key
   - Returns the public key for the master key of a BIP-85 derived seed

4. Combined BIP-85 and BIP-32 derivation
   - First derives a child mnemonic via BIP-85
   - Then applies BIP-32 derivation to get the corresponding public key

***

### handleAppleCallback

> **handleAppleCallback**: (`code`, `state`, `inviteCode`) => `Promise`\<`void`\>

#### Parameters

##### code

`string`

##### state

`string`

##### inviteCode

`string`

#### Returns

`Promise`\<`void`\>

***

### handleAppleNativeSignIn

> **handleAppleNativeSignIn**: (`appleUser`, `inviteCode?`) => `Promise`\<`void`\>

#### Parameters

##### appleUser

`api.AppleUser`

##### inviteCode?

`string`

#### Returns

`Promise`\<`void`\>

***

### handleGitHubCallback

> **handleGitHubCallback**: (`code`, `state`, `inviteCode`) => `Promise`\<`void`\>

#### Parameters

##### code

`string`

##### state

`string`

##### inviteCode

`string`

#### Returns

`Promise`\<`void`\>

***

### handleGoogleCallback

> **handleGoogleCallback**: (`code`, `state`, `inviteCode`) => `Promise`\<`void`\>

#### Parameters

##### code

`string`

##### state

`string`

##### inviteCode

`string`

#### Returns

`Promise`\<`void`\>

***

### initiateAppleAuth

> **initiateAppleAuth**: (`inviteCode`) => `Promise`\<`api.AppleAuthResponse`\>

#### Parameters

##### inviteCode

`string`

#### Returns

`Promise`\<`api.AppleAuthResponse`\>

***

### initiateGitHubAuth

> **initiateGitHubAuth**: (`inviteCode`) => `Promise`\<[`GithubAuthResponse`](GithubAuthResponse.md)\>

#### Parameters

##### inviteCode

`string`

#### Returns

`Promise`\<[`GithubAuthResponse`](GithubAuthResponse.md)\>

***

### initiateGoogleAuth

> **initiateGoogleAuth**: (`inviteCode`) => `Promise`\<[`GoogleAuthResponse`](GoogleAuthResponse.md)\>

#### Parameters

##### inviteCode

`string`

#### Returns

`Promise`\<[`GoogleAuthResponse`](GoogleAuthResponse.md)\>

***

### list

> **list**: *typeof* `api.fetchList`

Retrieves all key-value pairs stored by the user

#### Returns

A promise resolving to an array of stored items

#### Throws

If the list cannot be retrieved

- Calls the authenticated API endpoint to fetch all stored items
- Returns an array of key-value pairs with metadata
- Requires an active authentication session
- Each item includes key, value, creation, and update timestamps
- Logs any listing errors

***

### listApiKeys

> **listApiKeys**: *typeof* [`listApiKeys`](../functions/listApiKeys.md)

Lists all API keys for the authenticated user

#### Returns

A promise resolving to an object containing an array of API key metadata (without the actual keys)

#### Throws

If the user is not authenticated or the request fails

Returns metadata about all API keys associated with the user's account.
Note that the actual key values are never returned - they are only shown once during creation.
The keys are sorted by created_at in descending order (newest first).

***

### listConversationItems

> **listConversationItems**: *typeof* [`listConversationItems`](../functions/listConversationItems.md)

Lists items in a single conversation.

***

### listConversationProjects

> **listConversationProjects**: *typeof* [`listConversationProjects`](../functions/listConversationProjects.md)

Lists conversation projects with pagination.

***

### listConversations

> **listConversations**: *typeof* [`listConversations`](../functions/listConversations.md)

Lists all conversations with pagination (non-standard endpoint)

#### Param

Optional pagination parameters

#### Returns

A promise resolving to a paginated list of conversations

#### Description

This is a custom extension not part of the standard OpenAI API.
For standard conversation operations, use the OpenAI client directly:
- openai.conversations.create()
- openai.conversations.retrieve()
- openai.conversations.update()
- openai.conversations.delete()
- openai.conversations.items.list()
- openai.conversations.items.retrieve()

***

### listInstructions

> **listInstructions**: *typeof* `api.listInstructions`

Lists user's instructions with pagination

#### Param

Optional parameters for pagination and ordering

#### Returns

A promise resolving to a paginated list of instructions

#### Throws

If the user is not authenticated or the request fails

#### Description

Fetches a paginated list of the user's instructions.
Results are ordered by updated_at by default (most recently updated first).

***

### parseAttestationForView

> **parseAttestationForView**: (`document`, `cabundle`, `pcrConfig?`) => `Promise`\<[`ParsedAttestationView`](ParsedAttestationView.md)\>

Parses an attestation document for viewing

#### Parameters

##### document

[`AttestationDocument`](AttestationDocument.md)

##### cabundle

`Uint8Array`[]

##### pcrConfig?

[`PcrConfig`](PcrConfig.md)

#### Returns

`Promise`\<[`ParsedAttestationView`](ParsedAttestationView.md)\>

***

### pcrConfig

> **pcrConfig**: [`PcrConfig`](PcrConfig.md)

PCR0 trust policy enforced before every non-loopback session key exchange.
The trust environment defaults to production.

***

### put

> **put**: *typeof* `api.fetchPut`

Stores a key-value pair in the user's storage

#### Param

The unique identifier for the value

#### Param

The string value to be stored

#### Returns

A promise resolving to the server's response

#### Throws

If the value cannot be stored

- Calls the authenticated API endpoint to store a value
- Requires an active authentication session
- Overwrites any existing value for the given key
- Logs any storage errors

***

### refetchUser

> **refetchUser**: () => `Promise`\<`void`\>

#### Returns

`Promise`\<`void`\>

***

### refreshAccessToken

> **refreshAccessToken**: *typeof* `api.refreshToken`

***

### requestAccountDeletion

> **requestAccountDeletion**: (`hashedSecret`) => `Promise`\<`void`\>

Initiates the account deletion process for logged-in users

#### Parameters

##### hashedSecret

`string`

Client-side hashed secret for verification

#### Returns

`Promise`\<`void`\>

A promise resolving to void

#### Throws

If request fails

This function:
1. Requires the user to be logged in (uses authenticatedApiCall)
2. Sends a verification email to the user's email address
3. The email contains a confirmation code that will be needed for confirmation
4. The client must store the plaintext secret for confirmation

***

### requestNewVerificationCode

> **requestNewVerificationCode**: *typeof* `api.requestNewVerificationCode`

***

### requestNewVerificationEmail

> **requestNewVerificationEmail**: *typeof* `api.requestNewVerificationCode`

***

### requestPasswordReset

> **requestPasswordReset**: (`email`, `hashedSecret`) => `Promise`\<`void`\>

#### Parameters

##### email

`string`

##### hashedSecret

`string`

#### Returns

`Promise`\<`void`\>

***

### setApiKey

> **setApiKey**: (`key`) => `void`

Sets the API key to use for OpenAI endpoints.

#### Parameters

##### key

`string` \| `undefined`

The API key (UUID format) or undefined to clear

#### Returns

`void`

***

### setDefaultInstruction

> **setDefaultInstruction**: *typeof* `api.setDefaultInstruction`

Sets an instruction as the default

#### Param

The UUID of the instruction to set as default

#### Returns

A promise resolving to the updated instruction

#### Throws

If the instruction is not found

#### Description

Sets the specified instruction as the default.
All other instructions for this user are automatically set to is_default: false.
This operation is idempotent.

***

### signIn

> **signIn**: (`email`, `password`) => `Promise`\<`void`\>

Authenticates a user with email and password.

- Calls the login API endpoint with the configured clientId
- Stores access_token and refresh_token in localStorage
- Updates the auth state with user information
- Throws an error if authentication fails

#### Parameters

##### email

`string`

User's email address

##### password

`string`

User's password

#### Returns

`Promise`\<`void`\>

A promise that resolves when authentication is complete

#### Throws

If login fails

***

### signInGuest

> **signInGuest**: (`id`, `password`) => `Promise`\<`void`\>

Authenticates a guest user with user id and password

#### Parameters

##### id

`string`

User's unique id

##### password

`string`

User's password

#### Returns

`Promise`\<`void`\>

A promise that resolves when authentication is complete

#### Throws

If login fails

- Calls the login API endpoint
- Stores access_token and refresh_token in localStorage
- Updates the auth state with user information
- Throws an error if authentication fails

***

### signMessage

> **signMessage**: *typeof* `api.signMessage`

Signs a message using the specified algorithm.
This function supports multiple signing approaches: master key (no derivation),
BIP-32 derived key, BIP-85 derived key, or combined BIP-85 and BIP-32 derivation.

#### Param

The message to sign as a Uint8Array

#### Param

The signing algorithm ('schnorr' or 'ecdsa')

#### Param

Optional key derivation options or legacy BIP32 derivation path string

#### Returns

A promise resolving to the signature response

#### Throws

If the message signing fails

***

### signOut

> **signOut**: () => `Promise`\<`void`\>

Logs out the current user

#### Returns

`Promise`\<`void`\>

A promise that resolves when logout is complete

#### Throws

If logout fails

- Calls the logout API endpoint with the current refresh_token
- Removes access_token, refresh_token from localStorage
- Removes session-related items from sessionStorage
- Resets the auth state to show no user is authenticated

***

### signUp

> **signUp**: (`email`, `password`, `inviteCode`, `name?`) => `Promise`\<`void`\>

Creates a new user account

#### Parameters

##### email

`string`

User's email address

##### password

`string`

User's chosen password

##### inviteCode

`string`

Invitation code for registration

##### name?

`string`

Optional user's full name

#### Returns

`Promise`\<`void`\>

A promise that resolves when account creation is complete

#### Throws

If signup fails

- Calls the registration API endpoint
- Stores access_token and refresh_token in localStorage
- Updates the auth state with new user information
- Throws an error if account creation fails

***

### signUpGuest

> **signUpGuest**: (`password`, `inviteCode`) => `Promise`\<[`LoginResponse`](LoginResponse.md)\>

Creates a new long-lived guest account with no email recovery.

#### Parameters

##### password

`string`

User's chosen password. It can be changed while authenticated, but it cannot be recovered via email unless an email address is later added to the account.

##### inviteCode

`string`

Invitation code for registration

#### Returns

`Promise`\<[`LoginResponse`](LoginResponse.md)\>

A promise that resolves to the login response containing the guest ID

#### Throws

If signup fails

- Calls the registration API endpoint
- Stores access_token and refresh_token in localStorage
- Updates the auth state with new user information
- Throws an error if account creation fails

***

### transcribeAudio

> **transcribeAudio**: *typeof* `api.transcribeAudio`

Transcribes audio using the Whisper API

#### Param

The audio file to transcribe (File or Blob object)

#### Param

Optional transcription parameters

#### Returns

A promise resolving to the transcription response

#### Throws

If the user is not authenticated or transcription fails

#### Description

This function transcribes audio using OpenAI's Whisper model via the encrypted API.

Options:
- model: Model to use (default: "whisper-large-v3", routes to Tinfoil's whisper-large-v3-turbo)
- language: Optional ISO-639-1 language code (e.g., "en", "es", "fr")
- prompt: Optional context or previous segment transcript
- temperature: Sampling temperature between 0 and 1 (default: 0.0)

Supported audio formats: MP3, WAV, MP4, M4A, FLAC, OGG, WEBM

Example usage:
```typescript
const audioFile = new File([audioData], "recording.mp3", { type: "audio/mpeg" });
const result = await context.transcribeAudio(audioFile, {
  language: "en",
  prompt: "This is a technical discussion about AI"
});
console.log(result.text);
```

***

### updateConversation

> **updateConversation**: *typeof* [`updateConversation`](../functions/updateConversation.md)

Updates a single conversation's metadata/project/pin state.

***

### updateConversationProject

> **updateConversationProject**: *typeof* [`updateConversationProject`](../functions/updateConversationProject.md)

Updates a conversation project and/or its project-scoped instructions.

***

### updateInstruction

> **updateInstruction**: *typeof* `api.updateInstruction`

Updates an existing instruction

#### Param

The UUID of the instruction to update

#### Param

The fields to update

#### Returns

A promise resolving to the updated instruction

#### Throws

If the instruction is not found or validation fails

#### Description

At least one field must be provided.
If is_default: true is set, all other instructions are automatically set to is_default: false.
The prompt_tokens field is recalculated automatically if prompt changes.

***

### uploadDocument

> **uploadDocument**: (`file`) => `Promise`\<[`DocumentUploadInitResponse`](DocumentUploadInitResponse.md)\>

Uploads a document for text extraction and processing

#### Parameters

##### file

`File` \| `Blob`

The file to upload (File or Blob object)

#### Returns

`Promise`\<[`DocumentUploadInitResponse`](DocumentUploadInitResponse.md)\>

A promise resolving to the task ID and initial metadata

#### Throws

If:
- The file exceeds 10MB size limit
- The user is not authenticated (or is a guest user)
- Usage limits are exceeded (403)
- Processing fails (500)

#### Description

This function uploads a document to the Tinfoil processing service which:
1. Accepts the document and returns a task ID immediately
2. Processes the document asynchronously in the background
3. Maintains end-to-end encryption using session keys

Common supported formats include PDF, DOCX, XLSX, PPTX, TXT, RTF, and more.
Guest users will receive a 401 Unauthorized error.

Example usage:
```typescript
const file = new File(["content"], "document.pdf", { type: "application/pdf" });
const result = await context.uploadDocument(file);
console.log(result.task_id); // Task ID to check status
```

***

### uploadDocumentWithPolling

> **uploadDocumentWithPolling**: (`file`, `options?`) => `Promise`\<[`DocumentResponse`](DocumentResponse.md)\>

Uploads a document and polls for completion

#### Parameters

##### file

`File` \| `Blob`

The file to upload (File or Blob object)

##### options?

Optional configuration for polling behavior

###### maxAttempts?

`number`

###### onProgress?

(`status`, `progress?`) => `void`

###### pollInterval?

`number`

#### Returns

`Promise`\<[`DocumentResponse`](DocumentResponse.md)\>

A promise resolving to the processed document

#### Throws

If:
- Upload fails (see uploadDocument errors)
- Processing fails (error from server)
- Processing times out (exceeds maxAttempts)

#### Description

This is a convenience function that combines uploadDocument and checkDocumentStatus
to provide a simple interface that handles the async processing automatically.

Options:
- pollInterval: Time between status checks in milliseconds (default: 2000)
- maxAttempts: Maximum number of status checks before timeout (default: 150 = 5 minutes)
- onProgress: Callback function called on each status update

Example usage:
```typescript
const file = new File(["content"], "document.pdf", { type: "application/pdf" });
const result = await context.uploadDocumentWithPolling(file, {
  onProgress: (status, progress) => {
    console.log(`Status: ${status}, Progress: ${progress || 0}%`);
  }
});
console.log(result.text);
```

***

### verifyEmail

> **verifyEmail**: *typeof* `api.verifyEmail`
