import { describe, expect, test } from "bun:test";
import { resolveOpenSecretAttestationEnvironment } from "./attestation";

describe("resolveOpenSecretAttestationEnvironment", () => {
  test("defaults to production", () => {
    expect(resolveOpenSecretAttestationEnvironment(undefined)).toBe("prod");
    expect(resolveOpenSecretAttestationEnvironment("")).toBe("prod");
    expect(resolveOpenSecretAttestationEnvironment("  ")).toBe("prod");
  });

  test("accepts only exact supported environments", () => {
    expect(resolveOpenSecretAttestationEnvironment("prod")).toBe("prod");
    expect(resolveOpenSecretAttestationEnvironment(" dev ")).toBe("dev");
  });

  test("rejects misspelled or unexpected policies", () => {
    expect(() => resolveOpenSecretAttestationEnvironment("production")).toThrow(
      'VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT must be "prod" or "dev"'
    );
    expect(() => resolveOpenSecretAttestationEnvironment("DEV")).toThrow(
      'VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT must be "prod" or "dev"'
    );
  });
});
