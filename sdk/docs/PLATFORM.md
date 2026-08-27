# Developer Platform API

The Developer Platform API allows developers to manage organizations, projects, secrets, and user access within the OpenSecret platform. This API is specifically designed for developers integrating OpenSecret into their applications and platforms.

## `OpenSecretDeveloper`

The `OpenSecretDeveloper` component is the provider for developer-specific platform operations. It requires the URL of the OpenSecret developer API.

```tsx
import { OpenSecretDeveloper } from "@opensecret/react";

function App() {
  return (
    <OpenSecretDeveloper
      apiUrl="https://developer.opensecret.cloud"
      pcrConfig={{ environment: "prod" }} // Optional for the official production origin
    >
      <YourApp />
    </OpenSecretDeveloper>
  );
}
```

## Types Reference

Here are the main types used throughout the Platform API:

```typescript
// Authentication Types
export type PlatformLoginResponse = {
  id: string;
  email: string;
  name?: string;
  access_token: string;
  refresh_token: string;
};

export type PlatformRefreshResponse = {
  access_token: string;
  refresh_token: string;
};

// User Types
export type PlatformUser = {
  id: string;
  email: string;
  name?: string;
  email_verified: boolean;
  created_at: string;
  updated_at: string;
};

export type PlatformOrg = {
  id: string;
  name: string;
  role?: string;
  created_at?: string;
  updated_at?: string;
};

export type MeResponse = {
  user: PlatformUser;
  organizations: PlatformOrg[];
};

// Organization Types
export type Organization = {
  id: string;
  name: string;
};

export type OrganizationMember = {
  user_id: string;
  role: string;
  name?: string;
};

// Project Types
export type Project = {
  id: string;
  client_id: string;
  name: string;
  description?: string;
  status: string;
  created_at: string;
};

// Project Settings Types
export type ProjectSecret = {
  key_name: string;
  created_at: string;
  updated_at: string;
};

export type ProjectSettings = {
  category: string;
  settings: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type EmailSettings = {
  provider: string;
  send_from: string;
  email_verification_url: string;
};

export type OAuthProviderSettings = {
  client_id: string;
  redirect_url: string;
};

export type OAuthSettings = {
  google_oauth_enabled: boolean;
  github_oauth_enabled: boolean;
  apple_oauth_enabled: boolean;
  google_oauth_settings?: OAuthProviderSettings;
  github_oauth_settings?: OAuthProviderSettings;
  apple_oauth_settings?: OAuthProviderSettings;
};
```

## Developer Authentication

Before using the developer platform APIs, you need to authenticate. The SDK provides authentication methods through the `useOpenSecretDeveloper` hook:

```tsx
import { useOpenSecretDeveloper } from "@opensecret/react";

function DeveloperLogin() {
  const dev = useOpenSecretDeveloper();

  // Login with existing developer account
  async function handleLogin() {
    try {
      const response = await dev.signIn("developer@example.com", "yourpassword");
      console.log("Login successful", response);
      // Now you can use the developer context APIs
      // Authentication state is automatically updated
    } catch (error) {
      console.error("Login failed:", error);
    }
  }

  // Register a new developer account
  async function handleRegister() {
    try {
      const response = await dev.signUp(
        "developer@example.com",
        "yourpassword",
        "01234567-89ab-cdef-0123-456789abcdef", // Required invite code in UUID format
        "Developer Name" // Optional
      );
      console.log("Registration successful", response);
      // Now you can use the developer context APIs
      // Authentication state is automatically updated
    } catch (error) {
      console.error("Registration failed:", error);
    }
  }

  // Sign out
  async function handleLogout() {
    try {
      await dev.signOut();
      // The developer context will update automatically
    } catch (error) {
      console.error("Logout failed:", error);
    }
  }

  // Email verification
  async function handleVerifyEmail(code: string) {
    try {
      await dev.verifyEmail(code);
      console.log("Email verified successfully");
      // Reload developer info to update email_verified status
      await dev.refetchDeveloper();
    } catch (error) {
      console.error("Email verification failed:", error);
    }
  }

  // Request new verification email
  async function handleRequestNewVerificationEmail() {
    try {
      const response = await dev.requestNewVerificationEmail();
      console.log(response.message);
    } catch (error) {
      console.error("Failed to request new verification email:", error);
    }
  }

  return (
    <div>
      {/* Login/Register UI */}
    </div>
  );
}
```

### Authentication API Types and Methods

```typescript
// Authentication Methods
platformLogin(email: string, password: string): Promise<PlatformLoginResponse>
platformRegister(email: string, password: string, invite_code: string, name?: string): Promise<PlatformLoginResponse>
platformLogout(refresh_token: string): Promise<void>
platformRefreshToken(): Promise<PlatformRefreshResponse>
platformMe(): Promise<MeResponse>

// Email Verification Methods
verifyPlatformEmail(code: string): Promise<void>
requestNewPlatformVerificationCode(): Promise<{ message: string }>

// Password Management Methods
requestPlatformPasswordReset(email: string, hashedSecret: string): Promise<void>
confirmPlatformPasswordReset(
  email: string,
  alphanumericCode: string,
  plaintextSecret: string,
  newPassword: string
): Promise<{ message: string }>
changePlatformPassword(currentPassword: string, newPassword: string): Promise<{ message: string }>
```

### Password Management

The SDK provides methods for managing platform developer passwords, including password reset and password change functionality:

```tsx
import { useOpenSecretDeveloper } from "@opensecret/react";
import { generateSecureSecret, hashSecret } from "@opensecret/react";

function DeveloperPasswordManagement() {
  const dev = useOpenSecretDeveloper();

  // Request password reset
  async function handleRequestPasswordReset(email: string) {
    try {
      // Generate a secure random secret and its hash for verification
      const plaintextSecret = generateSecureSecret();
      const hashedSecret = hashSecret(plaintextSecret);

      // Store the plaintext secret securely (e.g., in component state)
      // to be used in the confirmation step
      setStoredSecret(plaintextSecret);

      // Request the password reset
      await dev.requestPasswordReset(email, hashedSecret);
      console.log("Password reset email sent. Check your inbox for the code.");
    } catch (error) {
      console.error("Failed to request password reset:", error);
    }
  }

  // Confirm password reset with code from email
  async function handleConfirmPasswordReset(
    email: string,
    code: string,
    plaintextSecret: string,
    newPassword: string
  ) {
    try {
      const response = await dev.confirmPasswordReset(
        email,
        code,
        plaintextSecret,
        newPassword
      );
      console.log(response.message);
      // User can now log in with the new password
    } catch (error) {
      console.error("Failed to confirm password reset:", error);
    }
  }

  // Change password when already logged in
  async function handleChangePassword(currentPassword: string, newPassword: string) {
    try {
      const response = await dev.changePassword(currentPassword, newPassword);
      console.log(response.message);
      // Password has been updated
    } catch (error) {
      console.error("Failed to change password:", error);
    }
  }

  return (
    <div>
      {/* Password Management UI */}
    </div>
  );
}
```

When a developer successfully logs in or registers, the authentication tokens are stored in localStorage and managed by the SDK. The `OpenSecretDeveloper` provider automatically detects these tokens and loads the developer profile. You can check the authentication state using the `auth` property:

```tsx
const dev = useOpenSecretDeveloper();

// Check if developer is loaded and authenticated
if (!dev.auth.loading && dev.auth.developer) {
  console.log("Developer is authenticated:", dev.auth.developer.email);
} else if (!dev.auth.loading) {
  console.log("Developer is not authenticated");
}
```

## `useOpenSecretDeveloper`

The `useOpenSecretDeveloper` hook provides access to all developer platform management APIs. It returns an object with the following properties and methods:

```tsx
import { useOpenSecretDeveloper } from "@opensecret/react";

function PlatformManagement() {
  const dev = useOpenSecretDeveloper();

  // Access developer information
  const { loading, developer } = dev.auth;

  // Now you can use any of the platform management methods
  // ...
}
```

### Developer State

- `auth`: An object containing the current developer's information
  - `loading`: Boolean indicating whether developer information is being loaded
  - `developer`: Developer data (undefined if not logged in) including:
    - `id`: Developer's unique ID
    - `email`: Developer's email address
    - `name`: Developer's name (optional)
    - `organizations`: Array of organizations the developer belongs to
- `apiUrl`: The current OpenSecret developer API URL being used

### Developer Authentication

- `signIn(email: string, password: string): Promise<PlatformLoginResponse>`: Signs in a developer with the provided email and password. Returns a response containing access and refresh tokens. The authentication state is automatically updated.
- `signUp(email: string, password: string, invite_code: string, name?: string): Promise<PlatformLoginResponse>`: Registers a new developer account with the provided email, password, invite code (in UUID format), and optional name. Returns a response containing access and refresh tokens. The authentication state is automatically updated.
- `signOut(): Promise<void>`: Signs out the current developer by removing authentication tokens and making a server logout call.
- `refetchDeveloper(): Promise<void>`: Refreshes the developer's authentication state. Useful after making changes that affect developer profile or organization membership.
- `verifyEmail(code: string): Promise<void>`: Verifies a developer's email address using the verification code sent to their email. This method is used to complete the email verification process.
- `requestNewVerificationCode(): Promise<{ message: string }>`: Requests a new verification email to be sent to the developer's email address. Used when the original verification email was not received or expired.
- `requestNewVerificationEmail(): Promise<{ message: string }>`: Alias for `requestNewVerificationCode()` for consistency with the OpenSecretContext API.
- `requestPasswordReset(email: string, hashedSecret: string): Promise<void>`: Initiates the password reset process for a platform developer account. The hashedSecret is used for additional security verification and should be created using the `hashSecret` function.
- `confirmPasswordReset(email: string, alphanumericCode: string, plaintextSecret: string, newPassword: string): Promise<{ message: string }>`: Completes the password reset process for a platform developer account using the code received via email and the plaintext secret that corresponds to the hashed secret sent in the initial request.
- `changePassword(currentPassword: string, newPassword: string): Promise<{ message: string }>`: Changes the password for an authenticated platform developer account. Requires the user to be logged in and to provide their current password for verification.

### Attestation Verification

- `pcrConfig`: Selects the `"prod"` or `"dev"` trusted-release environment enforced before non-loopback session establishment. The complete PCR0/PCR1/PCR2 tuple must match the SDK's embedded, Sigstore-verified release snapshot; callers cannot add custom roots or runtime history URLs.
- `getAttestation`: Gets an attested session after enforcing the effective trusted-release policy before key exchange.
- `authenticate`: Authenticates an attestation document.
- `parseAttestationForView`: Parses an attestation document for viewing.
- `awsRootCertDer`: AWS root certificate in DER format.
- `expectedRootCertHash`: Expected hash of the AWS root certificate.
- `getAttestationDocument()`: Gets and verifies an attestation document from the enclave. This is a convenience function that:
  1. Fetches the attestation document with a random nonce
  2. Authenticates the document
  3. Parses it for viewing

## Organization Management

### Organization API Types

```typescript
export type Organization = {
  id: string;
  name: string;
};

export type OrganizationMember = {
  user_id: string;
  role: string;
  name?: string;
};
```

### Organization API Methods

- `createOrganization(name: string): Promise<Organization>`: Creates a new organization with the given name.
- `listOrganizations(): Promise<Organization[]>`: Lists all organizations the developer has access to.
- `deleteOrganization(orgId: string): Promise<void>`: Deletes an organization (requires owner role).

Example:
```tsx
const handleCreateOrg = async () => {
  try {
    const org = await dev.createOrganization("My New Organization");
    console.log("Created organization:", org);
  } catch (error) {
    console.error("Failed to create organization:", error);
  }
};
```

## Project Management

### Project API Types

```typescript
export type Project = {
  id: string;
  client_id: string;
  name: string;
  description?: string;
  status: string;
  created_at: string;
};
```

### Project API Methods

- `createProject(orgId: string, name: string, description?: string): Promise<Project>`: Creates a new project within an organization.
- `listProjects(orgId: string): Promise<Project[]>`: Lists all projects within an organization.
- `getProject(orgId: string, projectId: string): Promise<Project>`: Gets a single project by ID.
- `updateProject(orgId: string, projectId: string, updates: { name?: string; description?: string; status?: string }): Promise<Project>`: Updates project details.
- `deleteProject(orgId: string, projectId: string): Promise<void>`: Deletes a project.

Example:
```tsx
const handleCreateProject = async (orgId) => {
  try {
    const project = await dev.createProject(
      orgId,
      "My New Project",
      "A description of my project"
    );
    console.log("Created project:", project);
    // project.client_id can be used as the clientId for OpenSecretProvider
  } catch (error) {
    console.error("Failed to create project:", error);
  }
};

const handleGetProject = async (orgId, projectId) => {
  try {
    const project = await dev.getProject(orgId, projectId);
    console.log("Retrieved project details:", project);
  } catch (error) {
    console.error("Failed to get project:", error);
  }
};
```

## Project Secrets Management

### Project Secrets API Types

```typescript
export type ProjectSecret = {
  key_name: string;
  created_at: string;
  updated_at: string;
};
```

### Project Secrets API Methods

- `createProjectSecret(orgId: string, projectId: string, keyName: string, secret: string): Promise<ProjectSecret>`: Creates a new secret for a project. The secret must be base64 encoded.
- `listProjectSecrets(orgId: string, projectId: string): Promise<ProjectSecret[]>`: Lists all secrets for a project.
- `deleteProjectSecret(orgId: string, projectId: string, keyName: string): Promise<void>`: Deletes a project secret.

Example:
```tsx
import { encode } from "@stablelib/base64";

const handleCreateSecret = async (orgId, projectId) => {
  // Encode the secret
  const secretValue = "my-secret-value";
  const encodedSecret = encode(new TextEncoder().encode(secretValue));

  try {
    await dev.createProjectSecret(
      orgId,
      projectId,
      "API_KEY",
      encodedSecret
    );
    console.log("Secret created successfully");
  } catch (error) {
    console.error("Failed to create secret:", error);
  }
};
```

## Email Configuration

### Email Settings API Types

```typescript
export type EmailSettings = {
  provider: string;
  send_from: string;
  email_verification_url: string;
};
```

### Email Settings API Methods

- `getEmailSettings(orgId: string, projectId: string): Promise<EmailSettings>`: Gets email configuration for a project.
- `updateEmailSettings(orgId: string, projectId: string, settings: EmailSettings): Promise<EmailSettings>`: Updates email configuration.

Example:
```tsx
const handleUpdateEmailSettings = async (orgId, projectId) => {
  try {
    await dev.updateEmailSettings(orgId, projectId, {
      provider: "resend",
      send_from: "noreply@yourdomain.com",
      email_verification_url: "https://yourdomain.com/verify-email"
    });
    console.log("Email settings updated");
  } catch (error) {
    console.error("Failed to update email settings:", error);
  }
};
```

## OAuth Configuration

### OAuth Settings API Types

```typescript
export type OAuthProviderSettings = {
  client_id: string;
  redirect_url: string;
};

export type OAuthSettings = {
  google_oauth_enabled: boolean;
  github_oauth_enabled: boolean;
  apple_oauth_enabled: boolean;
  google_oauth_settings?: OAuthProviderSettings;
  github_oauth_settings?: OAuthProviderSettings;
  apple_oauth_settings?: OAuthProviderSettings;
};
```

### OAuth Settings API Methods

- `getOAuthSettings(orgId: string, projectId: string): Promise<OAuthSettings>`: Gets OAuth settings for a project.
- `updateOAuthSettings(orgId: string, projectId: string, settings: OAuthSettings): Promise<OAuthSettings>`: Updates OAuth configuration.

Example:
```tsx
const handleUpdateOAuthSettings = async (orgId, projectId) => {
  try {
    await dev.updateOAuthSettings(orgId, projectId, {
      google_oauth_enabled: true,
      github_oauth_enabled: false,
      apple_oauth_enabled: true,
      google_oauth_settings: {
        client_id: "your-google-client-id",
        redirect_url: "https://yourdomain.com/auth/google/callback"
      },
      apple_oauth_settings: {
        client_id: "your.apple.service.id",
        redirect_url: "https://yourdomain.com/auth/apple/callback",
        team_id: "YOURTEAMID",     // Apple Developer Team ID
        key_id: "YOURKEYID"       // Key ID for Sign in with Apple
      }
    });
    console.log("OAuth settings updated");
  } catch (error) {
    console.error("Failed to update OAuth settings:", error);
  }
};
```

## Developer Membership Management

### Membership API Types

```typescript
export type OrganizationMember = {
  user_id: string;
  role: string;
  name?: string;
};

export type OrganizationInvite = {
  code: string;  // UUID of the invite
  email: string;
  role: string;
  used: boolean;
  expires_at: string;
  created_at: string;
  updated_at: string;
  organization_name?: string;  // Name of the organization (included in getOrganizationInvite response)
};
```

### Membership API Methods

- `inviteDeveloper(orgId: string, email: string, role?: string): Promise<OrganizationInvite>`: Creates an invitation to join an organization.
- `listOrganizationMembers(orgId: string): Promise<OrganizationMember[]>`: Lists all members of an organization.
- `listOrganizationInvites(orgId: string): Promise<OrganizationInvite[]>`: Lists all pending invitations for an organization.
- `getOrganizationInvite(orgId: string, inviteCode: string): Promise<OrganizationInvite>`: Gets a specific invitation by UUID code.
- `deleteOrganizationInvite(orgId: string, inviteCode: string): Promise<{ message: string }>`: Deletes an invitation.
- `updateMemberRole(orgId: string, userId: string, role: string): Promise<OrganizationMember>`: Updates a member's role.
- `removeMember(orgId: string, userId: string): Promise<void>`: Removes a member from the organization.
- `acceptInvite(code: string): Promise<{ message: string }>`: Accepts an organization invitation.

Example:
```tsx
// Create an invitation
const handleInviteDeveloper = async (orgId) => {
  try {
    const invite = await dev.inviteDeveloper(
      orgId,
      "developer@example.com",
      "admin" // Possible roles: "owner", "admin", "developer", "viewer"
    );
    console.log("Invitation sent:", invite);
  } catch (error) {
    console.error("Failed to invite developer:", error);
  }
};

// List all invitations
const handleListInvites = async (orgId) => {
  try {
    const invites = await dev.listOrganizationInvites(orgId);
    console.log("Pending invitations:", invites);
  } catch (error) {
    console.error("Failed to list invitations:", error);
  }
};

// Get a specific invitation
const handleGetInvite = async (orgId, inviteCode) => {
  try {
    const invite = await dev.getOrganizationInvite(orgId, inviteCode);
    console.log("Invitation details:", invite);
    console.log(`This invitation is for organization: ${invite.organization_name}`);
  } catch (error) {
    console.error("Failed to get invitation:", error);
  }
};

// Delete an invitation
const handleDeleteInvite = async (orgId, inviteCode) => {
  try {
    const result = await dev.deleteOrganizationInvite(orgId, inviteCode);
    console.log(result.message);
  } catch (error) {
    console.error("Failed to delete invitation:", error);
  }
};

// Accept an invitation
const handleAcceptInvite = async (inviteCode) => {
  try {
    const result = await dev.acceptInvite(inviteCode);
    console.log(result.message);  // "Invite accepted successfully"
  } catch (error) {
    console.error("Failed to accept invitation:", error);
  }
};
```

## Complete Example

Here's a complete example of how to use the developer platform API:

```tsx
import React, { useEffect, useState } from 'react';
import { OpenSecretDeveloper, useOpenSecretDeveloper } from '@opensecret/react';

function DeveloperPortal() {
  const dev = useOpenSecretDeveloper();
  const [orgs, setOrgs] = useState([]);
  const [selectedOrg, setSelectedOrg] = useState(null);
  const [projects, setProjects] = useState([]);

  useEffect(() => {
    // Load organizations when developer is available
    if (!dev.auth.loading && dev.auth.developer) {
      loadOrganizations();
    }
  }, [dev.auth.loading, dev.auth.developer]);

  async function loadOrganizations() {
    try {
      const orgs = await dev.listOrganizations();
      setOrgs(orgs);
    } catch (error) {
      console.error("Failed to load organizations:", error);
    }
  }

  async function loadProjects(orgId) {
    try {
      const projects = await dev.listProjects(orgId);
      setProjects(projects);
    } catch (error) {
      console.error("Failed to load projects:", error);
    }
  }

  async function handleCreateProject() {
    if (!selectedOrg) return;

    try {
      await dev.createProject(
        selectedOrg.id,
        "New Project " + Date.now(),
        "Created from developer portal"
      );
      // Reload projects
      loadProjects(selectedOrg.id);
    } catch (error) {
      console.error("Failed to create project:", error);
    }
  }

  return (
    <div>
      <h1>Developer Portal</h1>
      {dev.auth.loading ? (
        <p>Loading...</p>
      ) : dev.auth.developer ? (
        <div>
          <p>Welcome, {dev.auth.developer.name || dev.auth.developer.email}</p>

          <h2>Your Organizations</h2>
          <ul>
            {orgs.map(org => (
              <li key={org.id}>
                {org.name}
                <button onClick={() => {
                  setSelectedOrg(org);
                  loadProjects(org.id);
                }}>
                  Select
                </button>
              </li>
            ))}
          </ul>
          <button onClick={() => {
            const name = prompt("Organization name:");
            if (name) dev.createOrganization(name).then(loadOrganizations);
          }}>
            Create Organization
          </button>

          {selectedOrg && (
            <div>
              <h2>Projects in {selectedOrg.name}</h2>
              <ul>
                {projects.map(project => (
                  <li key={project.id}>
                    {project.name} - Client ID: {project.client_id}
                    <button onClick={async () => {
                      try {
                        const projectDetails = await dev.getProject(selectedOrg.id, project.id);
                        alert(`Project details: ${JSON.stringify(projectDetails, null, 2)}`);
                      } catch (error) {
                        console.error("Failed to get project details:", error);
                      }
                    }}>
                      View Details
                    </button>
                  </li>
                ))}
              </ul>
              <button onClick={handleCreateProject}>Create Project</button>
            </div>
          )}
        </div>
      ) : (
        <p>Please log in to access developer features</p>
      )}
    </div>
  );
}

function App() {
  return (
    <OpenSecretDeveloper apiUrl="https://developer.opensecret.cloud">
      <DeveloperPortal />
    </OpenSecretDeveloper>
  );
}

export default App;
```
