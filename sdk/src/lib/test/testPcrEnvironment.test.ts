import { describe, expect, test } from "bun:test";
import { parseTestPcrEnvironment } from "./testPcrEnvironment";

describe("hosted test PCR environment", () => {
  test("defaults to prod when omitted", () => {
    expect(parseTestPcrEnvironment(undefined)).toBe("prod");
  });

  test("accepts exact prod and dev values", () => {
    expect(parseTestPcrEnvironment("prod")).toBe("prod");
    expect(parseTestPcrEnvironment("dev")).toBe("dev");
  });

  test("rejects empty, differently-cased, or unknown values", () => {
    for (const value of ["", "Prod", "production", " dev "]) {
      expect(() => parseTestPcrEnvironment(value)).toThrow(
        'VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT must be either "prod" or "dev"'
      );
    }
  });
});
