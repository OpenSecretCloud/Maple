const trimTrailingSlash = (value: string) => value.replace(/\/+$/, "");

export const APP_ORIGIN = trimTrailingSlash(
  import.meta.env.VITE_APP_ORIGIN || "https://trymaple.ai"
);

export const MARKETING_ORIGIN = trimTrailingSlash(
  import.meta.env.VITE_MARKETING_ORIGIN || "https://www.trymaple.ai"
);

export function appUrl(path = "/") {
  return new URL(path, `${APP_ORIGIN}/`).toString();
}

export function marketingUrl(path = "/") {
  return new URL(path, `${MARKETING_ORIGIN}/`).toString();
}
