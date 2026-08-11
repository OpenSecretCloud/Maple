[**@opensecret/react**](../README.md)

***

# Type Alias: OpenSecretDeveloperContextType

> **OpenSecretDeveloperContextType** = `object`

## Properties

### acceptInvite

> **acceptInvite**: (`code`) => `Promise`\<\{ `message`: `string`; \}\>

Accepts an organization invitation

#### Parameters

##### code

`string`

Invitation UUID code

#### Returns

`Promise`\<\{ `message`: `string`; \}\>

***

### apiUrl

> **apiUrl**: `string`

Returns the current OpenSecret developer API URL being used

***

### auth

> **auth**: [`OpenSecretDeveloperAuthState`](OpenSecretDeveloperAuthState.md)

***

### authenticate

> **authenticate**: *typeof* `authenticate`

Authenticates an attestation document

***

### awsRootCertDer

> **awsRootCertDer**: *typeof* `AWS_ROOT_CERT_DER`

AWS root certificate in DER format

***

### changePassword

> **changePassword**: *typeof* `platformApi.changePlatformPassword`

Changes password for a platform developer account

#### Param

Current password for verification

#### Param

New password to set

#### Returns

A promise that resolves when the password is successfully changed

#### Throws

If current password is incorrect or the request fails

- Requires the user to be authenticated
- Verifies the current password before allowing the change
- Updates to the new password if verification succeeds

***

### confirmPasswordReset

> **confirmPasswordReset**: *typeof* `platformApi.confirmPlatformPasswordReset`

Completes the password reset process for a platform developer account

#### Param

Developer's email address

#### Param

Code received via email

#### Param

The plaintext secret that corresponds to the hashed_secret sent in the request

#### Param

New password to set

#### Returns

A promise that resolves when the password is successfully reset

#### Throws

If the verification fails or the request is invalid

- Completes the password reset process using the code from the email
- Requires the plaintext_secret that matches the previously sent hashed_secret
- Sets the new password if all verification succeeds
- The user can then log in with the new password

***

### createOrganization

> **createOrganization**: (`name`) => `Promise`\<`Organization`\>

Creates a new organization

#### Parameters

##### name

`string`

Organization name

#### Returns

`Promise`\<`Organization`\>

A promise that resolves to the created organization

***

### createProject

> **createProject**: (`orgId`, `name`, `description?`) => `Promise`\<`Project`\>

Creates a new project within an organization

#### Parameters

##### orgId

`string`

Organization ID

##### name

`string`

Project name

##### description?

`string`

Optional project description

#### Returns

`Promise`\<`Project`\>

A promise that resolves to the project details including client ID

***

### createProjectSecret

> **createProjectSecret**: (`orgId`, `projectId`, `keyName`, `secret`) => `Promise`\<`ProjectSecret`\>

Creates a new secret for a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### keyName

`string`

Secret key name (must be alphanumeric)

##### secret

`string`

Secret value (must be base64 encoded by the caller)

Example:
```typescript
// To encode a string secret
import { encode } from "@stablelib/base64";
const encodedSecret = encode(new TextEncoder().encode("my-secret-value"));

// Now pass the encoded secret to the function
createProjectSecret(orgId, projectId, "mySecretKey", encodedSecret);
```

#### Returns

`Promise`\<`ProjectSecret`\>

***

### deleteOrganization

> **deleteOrganization**: (`orgId`) => `Promise`\<`void`\>

Deletes an organization (requires owner role)

#### Parameters

##### orgId

`string`

Organization ID

#### Returns

`Promise`\<`void`\>

***

### deleteOrganizationInvite

> **deleteOrganizationInvite**: (`orgId`, `inviteCode`) => `Promise`\<\{ `message`: `string`; \}\>

Deletes an invitation

#### Parameters

##### orgId

`string`

Organization ID

##### inviteCode

`string`

Invitation UUID code

#### Returns

`Promise`\<\{ `message`: `string`; \}\>

***

### deleteProject

> **deleteProject**: (`orgId`, `projectId`) => `Promise`\<`void`\>

Deletes a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<`void`\>

***

### deleteProjectSecret

> **deleteProjectSecret**: (`orgId`, `projectId`, `keyName`) => `Promise`\<`void`\>

Deletes a project secret

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### keyName

`string`

Secret key name

#### Returns

`Promise`\<`void`\>

***

### expectedRootCertHash

> **expectedRootCertHash**: *typeof* `EXPECTED_ROOT_CERT_HASH`

Expected hash of the AWS root certificate

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

### getEmailSettings

> **getEmailSettings**: (`orgId`, `projectId`) => `Promise`\<`EmailSettings`\>

Gets email configuration for a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<`EmailSettings`\>

***

### getOAuthSettings

> **getOAuthSettings**: (`orgId`, `projectId`) => `Promise`\<`OAuthSettings`\>

Gets OAuth settings for a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<`OAuthSettings`\>

***

### getOrganizationInvite

> **getOrganizationInvite**: (`orgId`, `inviteCode`) => `Promise`\<`OrganizationInvite`\>

Gets a specific invitation by code

#### Parameters

##### orgId

`string`

Organization ID

##### inviteCode

`string`

Invitation UUID code

#### Returns

`Promise`\<`OrganizationInvite`\>

***

### getProject

> **getProject**: (`orgId`, `projectId`) => `Promise`\<`Project`\>

Gets a single project by ID

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<`Project`\>

A promise resolving to the project details

***

### getPushSettings

> **getPushSettings**: (`orgId`, `projectId`) => `Promise`\<[`PushSettings`](PushSettings.md)\>

Gets push settings for a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<[`PushSettings`](PushSettings.md)\>

***

### inviteDeveloper

> **inviteDeveloper**: (`orgId`, `email`, `role?`) => `Promise`\<`OrganizationInvite`\>

Creates an invitation to join an organization

#### Parameters

##### orgId

`string`

Organization ID

##### email

`string`

Developer's email address

##### role?

`string`

Role to assign (defaults to "admin")

#### Returns

`Promise`\<`OrganizationInvite`\>

***

### listOrganizationInvites

> **listOrganizationInvites**: (`orgId`) => `Promise`\<`OrganizationInvite`[]\>

Lists all pending invitations for an organization

#### Parameters

##### orgId

`string`

Organization ID

#### Returns

`Promise`\<`OrganizationInvite`[]\>

***

### listOrganizationMembers

> **listOrganizationMembers**: (`orgId`) => `Promise`\<`OrganizationMember`[]\>

Lists all members of an organization

#### Parameters

##### orgId

`string`

Organization ID

#### Returns

`Promise`\<`OrganizationMember`[]\>

***

### listOrganizations

> **listOrganizations**: () => `Promise`\<`Organization`[]\>

Lists all organizations the developer has access to

#### Returns

`Promise`\<`Organization`[]\>

A promise resolving to array of organization details

***

### listProjects

> **listProjects**: (`orgId`) => `Promise`\<`Project`[]\>

Lists all projects within an organization

#### Parameters

##### orgId

`string`

Organization ID

#### Returns

`Promise`\<`Project`[]\>

A promise resolving to array of project details

***

### listProjectSecrets

> **listProjectSecrets**: (`orgId`, `projectId`) => `Promise`\<`ProjectSecret`[]\>

Lists all secrets for a project

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

#### Returns

`Promise`\<`ProjectSecret`[]\>

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

### refetchDeveloper

> **refetchDeveloper**: () => `Promise`\<`void`\>

Refreshes the developer's authentication state

#### Returns

`Promise`\<`void`\>

A promise that resolves when the refresh is complete

#### Throws

If the refresh fails

- Retrieves the latest developer information from the server
- Updates the developer state with fresh data
- Useful after making changes that affect developer profile or organization membership

***

### removeMember

> **removeMember**: (`orgId`, `userId`) => `Promise`\<`void`\>

Removes a member from the organization

#### Parameters

##### orgId

`string`

Organization ID

##### userId

`string`

User ID to remove

#### Returns

`Promise`\<`void`\>

***

### requestNewVerificationCode

> **requestNewVerificationCode**: *typeof* `platformApi.requestNewPlatformVerificationCode`

Requests a new verification email for the current user

#### Returns

A promise that resolves to a success message

#### Throws

If the user is already verified or request fails

- Used when the user needs a new verification email
- Requires the user to be authenticated
- Sends a new verification email to the user's registered email address

***

### requestNewVerificationEmail

> **requestNewVerificationEmail**: *typeof* `platformApi.requestNewPlatformVerificationCode`

Alias for requestNewVerificationCode - for consistency with OpenSecretContext

***

### requestPasswordReset

> **requestPasswordReset**: *typeof* `platformApi.requestPlatformPasswordReset`

Initiates the password reset process for a platform developer account

#### Param

Developer's email address

#### Param

Hashed secret used for additional security verification

#### Returns

A promise that resolves when the reset request is successfully processed

#### Throws

If the request fails or the email doesn't exist

- Sends a password reset request for a platform developer
- The server will send an email with an alphanumeric code
- The email and hashed_secret are paired for the reset process
- Use confirmPasswordReset to complete the process

***

### signIn

> **signIn**: (`email`, `password`) => `Promise`\<`platformApi.PlatformLoginResponse`\>

Signs in a developer with email and password

#### Parameters

##### email

`string`

Developer's email address

##### password

`string`

Developer's password

#### Returns

`Promise`\<`platformApi.PlatformLoginResponse`\>

A promise that resolves to the login response with access and refresh tokens

- Calls the login API endpoint
- Stores access_token and refresh_token in localStorage
- Updates the developer state with user information
- Throws an error if authentication fails

***

### signOut

> **signOut**: () => `Promise`\<`void`\>

Signs out the current developer by removing authentication tokens

- Calls the logout API endpoint with the current refresh_token
- Removes access_token, refresh_token from localStorage
- Resets the developer state to show no user is authenticated

#### Returns

`Promise`\<`void`\>

***

### signUp

> **signUp**: (`email`, `password`, `invite_code`, `name?`) => `Promise`\<`platformApi.PlatformLoginResponse`\>

Registers a new developer account

#### Parameters

##### email

`string`

Developer's email address

##### password

`string`

Developer's password

##### invite\_code

`string`

Required invitation code in UUID format

##### name?

`string`

Optional developer name

#### Returns

`Promise`\<`platformApi.PlatformLoginResponse`\>

A promise that resolves to the login response with access and refresh tokens

- Calls the registration API endpoint
- Stores access_token and refresh_token in localStorage
- Updates the developer state with new user information
- Throws an error if account creation fails

***

### updateEmailSettings

> **updateEmailSettings**: (`orgId`, `projectId`, `settings`) => `Promise`\<`EmailSettings`\>

Updates email configuration

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### settings

`EmailSettings`

Email settings

#### Returns

`Promise`\<`EmailSettings`\>

***

### updateMemberRole

> **updateMemberRole**: (`orgId`, `userId`, `role`) => `Promise`\<`OrganizationMember`\>

Updates a member's role

#### Parameters

##### orgId

`string`

Organization ID

##### userId

`string`

User ID to update

##### role

`string`

New role to assign

#### Returns

`Promise`\<`OrganizationMember`\>

***

### updateOAuthSettings

> **updateOAuthSettings**: (`orgId`, `projectId`, `settings`) => `Promise`\<`OAuthSettings`\>

Updates OAuth configuration

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### settings

`OAuthSettings`

OAuth settings

#### Returns

`Promise`\<`OAuthSettings`\>

***

### updateProject

> **updateProject**: (`orgId`, `projectId`, `updates`) => `Promise`\<`Project`\>

Updates project details

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### updates

Object containing fields to update

###### description?

`string`

###### name?

`string`

###### status?

`string`

#### Returns

`Promise`\<`Project`\>

***

### updatePushSettings

> **updatePushSettings**: (`orgId`, `projectId`, `settings`) => `Promise`\<[`PushSettings`](PushSettings.md)\>

Updates push configuration

#### Parameters

##### orgId

`string`

Organization ID

##### projectId

`string`

Project ID

##### settings

[`PushSettings`](PushSettings.md)

Push settings

#### Returns

`Promise`\<[`PushSettings`](PushSettings.md)\>

***

### verifyEmail

> **verifyEmail**: *typeof* `platformApi.verifyPlatformEmail`

Verifies a platform user's email using the verification code

#### Param

The verification code sent to the user's email

#### Returns

A promise that resolves when verification is complete

#### Throws

If verification fails

- Takes the verification code from the verification email link
- Calls the verification API endpoint
- Updates email_verified status if successful
