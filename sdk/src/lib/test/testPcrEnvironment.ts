import type { AttestationEnvironment } from "../pcr";

const PCR_ENVIRONMENT_VARIABLE = "VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT";

/** Parse the PCR trust environment used by hosted integration tests. */
export function parseTestPcrEnvironment(value: string | undefined): AttestationEnvironment {
  if (value === undefined) return "prod";
  if (value === "prod" || value === "dev") return value;

  throw new Error(`${PCR_ENVIRONMENT_VARIABLE} must be either "prod" or "dev"`);
}
