/**
 * Truncates text while preserving clickable links.
 * Converts plain URLs that would be truncated into markdown links with full href but truncated display.
 */
export function truncateMarkdownPreservingLinks(text: string, maxLength: number): string {
  if (text.length <= maxLength) {
    return text;
  }

  // Find all plain URLs in the text (not already in markdown link format)
  const urlRegex = /(https?:\/\/[^\s\])<>]+)/g;
  const urls: { start: number; end: number; url: string }[] = [];
  let match;

  while ((match = urlRegex.exec(text)) !== null) {
    // Check if this URL is inside a markdown link [text](url) - skip if so
    const beforeUrl = text.substring(0, match.index);
    const isInMarkdownLink =
      /\[[^\]]*\]?\($/.test(beforeUrl) || beforeUrl.lastIndexOf("](") > beforeUrl.lastIndexOf(")");
    if (!isInMarkdownLink) {
      urls.push({ start: match.index, end: match.index + match[0].length, url: match[0] });
    }
  }

  // Check if truncation point falls within a plain URL
  for (const urlInfo of urls) {
    if (maxLength > urlInfo.start && maxLength < urlInfo.end) {
      // Truncation would cut this URL - convert to markdown link with truncated display
      const beforeUrl = text.substring(0, urlInfo.start);
      const truncatedDisplay = urlInfo.url.substring(0, maxLength - urlInfo.start) + "...";
      // Create markdown link: [truncated-display](full-url)
      return beforeUrl + `[${truncatedDisplay}](${urlInfo.url})`;
    }
  }

  // Also handle existing markdown links
  const mdLinkRegex = /\[([^\]]*)\]\(([^)]+)\)/g;
  while ((match = mdLinkRegex.exec(text)) !== null) {
    if (maxLength > match.index && maxLength < match.index + match[0].length) {
      // Truncation would break this markdown link - truncate before it
      const truncated = text.substring(0, match.index).trimEnd();
      return truncated + "...";
    }
  }

  // Safe to truncate at maxLength
  return text.substring(0, maxLength) + "...";
}
