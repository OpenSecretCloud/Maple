import React from "react";
import { openExternalLink } from "@/utils/openExternalLink";

interface ExternalLinkProps
  extends Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "target" | "rel"> {
  href: string;
  children: React.ReactNode;
  fallbackBehavior?: "window.open" | "location.href";
}

export function ExternalLink({
  href,
  children,
  onClick,
  fallbackBehavior = "window.open",
  ...props
}: ExternalLinkProps) {
  const handleClick = async (e: React.MouseEvent<HTMLAnchorElement>) => {
    // Prevent default link behavior
    e.preventDefault();

    // Call any custom onClick handler
    if (onClick) {
      onClick(e);
    }

    // Open the link using our utility
    await openExternalLink(href, { fallbackBehavior });
  };

  return (
    <a href={href} onClick={handleClick} target="_blank" rel="noopener noreferrer" {...props}>
      {children}
    </a>
  );
}
