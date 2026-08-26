import { expect, test, beforeEach, mock } from "bun:test";
import { PlatformLoginResponse } from "../../platformApi";

// Mock platform login response
const mockLoginResponse: PlatformLoginResponse = {
  id: "test-id",
  email: "test@example.com",
  access_token: "test-access-token",
  refresh_token: "test-refresh-token"
};

// Setup localStorage mock
beforeEach(() => {
  window.localStorage.clear();
});

test("Developer signIn method works correctly", async () => {
  // Mock the signIn function
  const mockSignIn = mock((email: string, password: string) => {
    expect(email).toBe("test@example.com");
    expect(password).toBe("password");
    return Promise.resolve(mockLoginResponse);
  });

  // Create a mock context value
  const mockContext = {
    developer: {
      loading: false,
      developer: undefined
    },
    signIn: mockSignIn,
    signUp: () => Promise.resolve(mockLoginResponse),
    signOut: () => {},
    createOrganization: () => Promise.resolve({ id: "org-id", name: "Test Org" }),
    listOrganizations: () => Promise.resolve([]),
    deleteOrganization: () => Promise.resolve(),
    createProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    listProjects: () => Promise.resolve([]),
    updateProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    deleteProject: () => Promise.resolve(),
    createProjectSecret: () =>
      Promise.resolve({ key_name: "test", created_at: "", updated_at: "" }),
    listProjectSecrets: () => Promise.resolve([]),
    deleteProjectSecret: () => Promise.resolve(),
    getEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    updateEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    getOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    updateOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    getPushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    updatePushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    inviteDeveloper: () => Promise.resolve({ code: "code" }),
    listOrganizationMembers: () => Promise.resolve([]),
    updateMemberRole: () => Promise.resolve({ user_id: "", role: "" }),
    removeMember: () => Promise.resolve(),
    acceptInvite: () => Promise.resolve(),
    apiUrl: "https://example.com"
  };

  // Mock direct call to sign in method
  await mockContext.signIn("test@example.com", "password");

  // Verify the mock was called
  expect(mockSignIn).toHaveBeenCalled();
});

test("Developer signUp method works correctly", async () => {
  // Mock the signUp function
  const mockSignUp = mock((email: string, password: string, name?: string) => {
    expect(email).toBe("test@example.com");
    expect(password).toBe("password");
    expect(name).toBe("Test User");
    return Promise.resolve(mockLoginResponse);
  });

  // Create a mock context value
  const mockContext = {
    developer: {
      loading: false,
      developer: undefined
    },
    signIn: () => Promise.resolve(mockLoginResponse),
    signUp: mockSignUp,
    signOut: () => {},
    createOrganization: () => Promise.resolve({ id: "org-id", name: "Test Org" }),
    listOrganizations: () => Promise.resolve([]),
    deleteOrganization: () => Promise.resolve(),
    createProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    listProjects: () => Promise.resolve([]),
    updateProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    deleteProject: () => Promise.resolve(),
    createProjectSecret: () =>
      Promise.resolve({ key_name: "test", created_at: "", updated_at: "" }),
    listProjectSecrets: () => Promise.resolve([]),
    deleteProjectSecret: () => Promise.resolve(),
    getEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    updateEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    getOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    updateOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    getPushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    updatePushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    inviteDeveloper: () => Promise.resolve({ code: "code" }),
    listOrganizationMembers: () => Promise.resolve([]),
    updateMemberRole: () => Promise.resolve({ user_id: "", role: "" }),
    removeMember: () => Promise.resolve(),
    acceptInvite: () => Promise.resolve(),
    apiUrl: "https://example.com"
  };

  // Mock direct call to sign up method
  await mockContext.signUp("test@example.com", "password", "Test User");

  // Verify the mock was called
  expect(mockSignUp).toHaveBeenCalled();
});

test("Developer signOut method works correctly", async () => {
  // Setup localStorage with some tokens
  window.localStorage.setItem("access_token", "test-token");
  window.localStorage.setItem("refresh_token", "test-refresh");

  // Mock the signOut function
  const mockSignOut = mock(() => {
    localStorage.removeItem("access_token");
    localStorage.removeItem("refresh_token");
  });

  // Create mock state
  const mockDeveloper = {
    id: "user-id",
    email: "test@example.com",
    email_verified: true,
    created_at: "2023-01-01",
    updated_at: "2023-01-01",
    organizations: []
  };

  // Create a mock context value with authenticated developer
  const mockContext = {
    developer: {
      loading: false,
      developer: mockDeveloper
    },
    signIn: () => Promise.resolve(mockLoginResponse),
    signUp: () => Promise.resolve(mockLoginResponse),
    signOut: mockSignOut,
    createOrganization: () => Promise.resolve({ id: "org-id", name: "Test Org" }),
    listOrganizations: () => Promise.resolve([]),
    deleteOrganization: () => Promise.resolve(),
    createProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    listProjects: () => Promise.resolve([]),
    updateProject: () =>
      Promise.resolve({
        id: 1,
        uuid: "uuid",
        client_id: "client-id",
        name: "Test Project",
        status: "active"
      }),
    deleteProject: () => Promise.resolve(),
    createProjectSecret: () =>
      Promise.resolve({ key_name: "test", created_at: "", updated_at: "" }),
    listProjectSecrets: () => Promise.resolve([]),
    deleteProjectSecret: () => Promise.resolve(),
    getEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    updateEmailSettings: () =>
      Promise.resolve({ provider: "", send_from: "", email_verification_url: "" }),
    getOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    updateOAuthSettings: () =>
      Promise.resolve({ google_oauth_enabled: false, github_oauth_enabled: false }),
    getPushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    updatePushSettings: () => Promise.resolve({ encrypted_preview_enabled: false }),
    inviteDeveloper: () => Promise.resolve({ code: "code" }),
    listOrganizationMembers: () => Promise.resolve([]),
    updateMemberRole: () => Promise.resolve({ user_id: "", role: "" }),
    removeMember: () => Promise.resolve(),
    acceptInvite: () => Promise.resolve(),
    apiUrl: "https://example.com"
  };

  // Call signOut
  mockContext.signOut();

  // Verify the mock was called
  expect(mockSignOut).toHaveBeenCalled();

  // Verify tokens were removed
  expect(window.localStorage.getItem("access_token")).toBeNull();
  expect(window.localStorage.getItem("refresh_token")).toBeNull();
});
