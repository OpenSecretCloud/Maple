/**
 * Truncates text while preserving clickable links.
 * Converts plain URLs that would be truncated into markdown links with full href but truncated display.
 */
export function truncateMarkdownPreservingLinks(text: string, maxLength: number): string {
  if (text.length <= maxLength) {
    return text;
  }

  // Precompute all markdown link ranges first
  const mdLinkRegex = /\[([^\]]*)\]\(([^)]+)\)/g;
  const mdLinkRanges: { start: number; end: number }[] = [];
  let match;

  while ((match = mdLinkRegex.exec(text)) !== null) {
    mdLinkRanges.push({ start: match.index, end: match.index + match[0].length });
  }

  // Helper to check if an index falls inside any markdown link range
  const isInsideMdLink = (index: number): boolean => {
    return mdLinkRanges.some((range) => index >= range.start && index < range.end);
  };

  // Find all plain URLs that are not inside markdown links
  const urlRegex = /(https?:\/\/[^\s\]<>]+)/g;
  const plainUrls: { start: number; end: number; url: string }[] = [];

  while ((match = urlRegex.exec(text)) !== null) {
    if (!isInsideMdLink(match.index)) {
      plainUrls.push({ start: match.index, end: match.index + match[0].length, url: match[0] });
    }
  }

  // Check if truncation point falls within a plain URL
  for (const urlInfo of plainUrls) {
    if (maxLength > urlInfo.start && maxLength < urlInfo.end) {
      // Truncation would cut this URL - convert to markdown link with truncated display
      const beforeUrl = text.substring(0, urlInfo.start);
      const truncatedDisplay = urlInfo.url.substring(0, maxLength - urlInfo.start) + "...";
      // Create markdown link: [truncated-display](full-url)
      return beforeUrl + `[${truncatedDisplay}](${urlInfo.url})`;
    }
  }

  // Check if truncation point falls within a markdown link (reuse precomputed ranges)
  for (const range of mdLinkRanges) {
    if (maxLength > range.start && maxLength < range.end) {
      // Truncation would break this markdown link - truncate before it
      const truncated = text.substring(0, range.start).trimEnd();
      return truncated + "...";
    }
  }

  // Safe to truncate at maxLength
  return text.substring(0, maxLength) + "...";
}
