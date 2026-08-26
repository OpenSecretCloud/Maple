import type { PcrEnvironment } from "../pcr";

const PCR_ENVIRONMENT_VARIABLE = "VITE_OPEN_SECRET_PCR_ENVIRONMENT";

/** Parse the PCR trust environment used by hosted integration tests. */
export function parseTestPcrEnvironment(value: string | undefined): PcrEnvironment {
  if (value === undefined) return "production";
  if (value === "production" || value === "development") return value;

  throw new Error(`${PCR_ENVIRONMENT_VARIABLE} must be either "production" or "development"`);
}
