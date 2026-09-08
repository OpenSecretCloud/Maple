import { useEffect } from "react";

type RouteMetaOptions = {
  title: string;
  description: string;
  canonicalUrl?: string;
  robots?: string;
};

function setMeta(selector: string, attributes: Record<string, string>) {
  let element = document.head.querySelector<HTMLMetaElement>(selector);

  if (!element) {
    element = document.createElement("meta");
    const name = attributes.name ?? attributes.property;
    if (attributes.name) element.setAttribute("name", name);
    if (attributes.property) element.setAttribute("property", name);
    document.head.appendChild(element);
  }

  Object.entries(attributes).forEach(([key, value]) => {
    element?.setAttribute(key, value);
  });
}

function setCanonical(canonicalUrl?: string) {
  const existing = document.head.querySelector<HTMLLinkElement>('link[rel="canonical"]');

  if (!canonicalUrl) {
    existing?.remove();
    return;
  }

  const element = existing ?? document.createElement("link");
  element.setAttribute("rel", "canonical");
  element.setAttribute("href", canonicalUrl);

  if (!existing) {
    document.head.appendChild(element);
  }
}

function removeMeta(selector: string) {
  document.head.querySelector<HTMLMetaElement>(selector)?.remove();
}

export function useRouteMeta({
  title,
  description,
  canonicalUrl,
  robots = "noindex, follow"
}: RouteMetaOptions) {
  useEffect(() => {
    document.title = title;

    setMeta('meta[name="description"]', { name: "description", content: description });
    setMeta('meta[name="robots"]', { name: "robots", content: robots });
    setMeta('meta[property="og:title"]', { property: "og:title", content: title });
    setMeta('meta[property="og:description"]', {
      property: "og:description",
      content: description
    });
    setMeta('meta[name="twitter:title"]', { name: "twitter:title", content: title });
    setMeta('meta[name="twitter:description"]', {
      name: "twitter:description",
      content: description
    });

    if (canonicalUrl) {
      setMeta('meta[property="og:url"]', { property: "og:url", content: canonicalUrl });
    } else {
      removeMeta('meta[property="og:url"]');
    }

    setCanonical(canonicalUrl);
  }, [canonicalUrl, description, robots, title]);
}
