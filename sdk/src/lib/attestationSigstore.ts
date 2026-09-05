// The release-policy client always imports the production verifier through this
// fixed binding. Bun tests replace this module at the loader boundary, so no
// verifier replacement seam is compiled into the shipped SDK.
export { verifyTufAuthorizedSigstoreBundle } from "./sigstoreBrowser";
