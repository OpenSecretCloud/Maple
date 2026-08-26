import { describe, expect, test } from "bun:test";
import { parseTestPcrEnvironment } from "./testPcrEnvironment";

describe("hosted test PCR environment", () => {
  test("defaults to production when omitted", () => {
    expect(parseTestPcrEnvironment(undefined)).toBe("production");
  });

  test("accepts exact production and development values", () => {
    expect(parseTestPcrEnvironment("production")).toBe("production");
    expect(parseTestPcrEnvironment("development")).toBe("development");
  });

  test("rejects empty, differently-cased, or unknown values", () => {
    for (const value of ["", "Production", "dev", " development "]) {
      expect(() => parseTestPcrEnvironment(value)).toThrow(
        'VITE_OPEN_SECRET_PCR_ENVIRONMENT must be either "production" or "development"'
      );
    }
  });
});
