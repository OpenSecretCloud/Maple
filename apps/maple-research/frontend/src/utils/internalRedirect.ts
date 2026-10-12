const INTERNAL_REDIRECT_ORIGIN = "https://maple.internal";

type InternalNavigationHistory = {
  push: (href: string) => void;
  replace: (href: string) => void;
};

export function getSafeInternalRedirect(value: unknown): string | undefined {
  if (typeof value !== "string" || !value.startsWith("/") || value.includes("\\")) {
    return undefined;
  }

  try {
    const resolved = new URL(value, INTERNAL_REDIRECT_ORIGIN);
    if (resolved.origin !== INTERNAL_REDIRECT_ORIGIN) {
      return undefined;
    }

    const redirect = `${resolved.pathname}${resolved.search}${resolved.hash}`;
    if (!redirect.startsWith("/") || redirect.startsWith("//") || redirect.includes("\\")) {
      return undefined;
    }

    return redirect;
  } catch {
    return undefined;
  }
}

export function navigateToSafeInternalRedirect(
  history: InternalNavigationHistory,
  value: unknown,
  { replace = false }: { replace?: boolean } = {}
): boolean {
  const redirect = getSafeInternalRedirect(value);
  if (!redirect) return false;

  if (replace) {
    history.replace(redirect);
  } else {
    history.push(redirect);
  }
  return true;
}

/** Carry an entry page's destination through either login or signup. */
export function getAppEntryRedirect(href: string): string | undefined {
  const safeHref = getSafeInternalRedirect(href);
  if (!safeHref) return undefined;
  const location = new URL(safeHref, INTERNAL_REDIRECT_ORIGIN);
  const next = getSafeInternalRedirect(location.searchParams.get("next"));
  if (next) return next;

  // Entry controls must not send a newly authenticated user back to login.
  location.searchParams.delete("login");
  location.searchParams.delete("next");
  const destination = `${location.pathname}${location.search}${location.hash}`;
  return destination === "/" ? undefined : destination;
}
