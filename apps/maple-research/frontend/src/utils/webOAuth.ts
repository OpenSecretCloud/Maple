/** Keep browser OAuth on the origin that owns this tab's continuation state. */
export function webOAuthCallbackUrl(provider: "github" | "google" | "apple"): string {
  const { origin, protocol } = window.location;
  if (protocol !== "https:" && protocol !== "http:") {
    throw new Error("Browser OAuth requires an HTTP origin");
  }
  return `${origin}/auth/${provider}/callback`;
}
