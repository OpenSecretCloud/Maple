export type OpenSecretAttestationEnvironment = "prod" | "dev";

export function resolveOpenSecretAttestationEnvironment(
  configuredEnvironment: string | undefined
): OpenSecretAttestationEnvironment {
  const environment = configuredEnvironment?.trim();

  if (!environment || environment === "prod") {
    return "prod";
  }

  if (environment === "dev") {
    return "dev";
  }

  throw new Error(
    `VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT must be "prod" or "dev", received ${JSON.stringify(environment)}`
  );
}

export const OPEN_SECRET_ATTESTATION_ENVIRONMENT = resolveOpenSecretAttestationEnvironment(
  import.meta.env.VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT
);
