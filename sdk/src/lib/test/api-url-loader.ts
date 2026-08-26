import { setApiUrl } from "../api";
import { parseTestPcrEnvironment } from "./testPcrEnvironment";

// Get the API URL from environment variables
const apiUrl = process.env.VITE_OPEN_SECRET_API_URL;
const pcrEnvironment = parseTestPcrEnvironment(process.env.VITE_OPEN_SECRET_PCR_ENVIRONMENT);

if (!apiUrl) {
  throw new Error("VITE_OPEN_SECRET_API_URL must be set in environment variables");
}

// Bind the hosted endpoint to one explicit PCR trust environment before tests run.
setApiUrl(apiUrl, { environment: pcrEnvironment });

console.log("API URL set to:", apiUrl);
console.log("PCR trust environment set to:", pcrEnvironment);
