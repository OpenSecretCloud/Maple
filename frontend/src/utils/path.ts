const DEFAULT_PATH_TRUNCATION_LENGTH = 48;
const VISIBLE_SEGMENT_CANDIDATES = [
  [2, 2],
  [1, 2],
  [2, 1],
  [1, 1]
] as const;

function characterCount(value: string): number {
  return Array.from(value).length;
}

function truncateCharactersMiddle(value: string, maxLength: number): string {
  const characters = Array.from(value);
  if (characters.length <= maxLength) return value;
  if (maxLength <= 0) return "";
  if (maxLength === 1) return "…";

  const visibleCharacterCount = maxLength - 1;
  const leadingCharacterCount = Math.ceil(visibleCharacterCount / 2);
  const trailingCharacterCount = Math.floor(visibleCharacterCount / 2);
  const leadingCharacters = characters.slice(0, leadingCharacterCount).join("");
  const trailingCharacters =
    trailingCharacterCount > 0 ? characters.slice(-trailingCharacterCount).join("") : "";

  return `${leadingCharacters}…${trailingCharacters}`;
}

function pathSeparator(path: string): "/" | "\\" {
  const driveSeparator = path.match(/^[A-Za-z]:([\\/])/);
  if (driveSeparator?.[1] === "\\") return "\\";
  if (driveSeparator?.[1] === "/") return "/";
  if (path.startsWith("\\\\") || (!path.includes("/") && path.includes("\\"))) return "\\";
  return "/";
}

function pathPrefixLength(path: string): number {
  if (/^[A-Za-z]:[\\/]/.test(path)) return 3;
  if (path.startsWith("\\\\") || path.startsWith("//")) return 2;
  if (path.startsWith("/") || path.startsWith("\\")) return 1;
  return 0;
}

export function truncatePathMiddle(
  path: string,
  maxLength = DEFAULT_PATH_TRUNCATION_LENGTH
): string {
  const normalizedMaxLength = Number.isFinite(maxLength)
    ? Math.max(0, Math.floor(maxLength))
    : DEFAULT_PATH_TRUNCATION_LENGTH;
  if (characterCount(path) <= normalizedMaxLength) return path;

  const separator = pathSeparator(path);
  const prefixLength = pathPrefixLength(path);
  const prefix = path.slice(0, prefixLength);
  const remainder = path.slice(prefixLength);
  const hasTrailingSeparator = /[\\/]$/.test(remainder);
  const segments = remainder.split(/[\\/]+/).filter(Boolean);

  for (const [leadingSegmentCount, trailingSegmentCount] of VISIBLE_SEGMENT_CANDIDATES) {
    if (leadingSegmentCount + trailingSegmentCount >= segments.length) continue;

    const shortenedPath = `${prefix}${[
      ...segments.slice(0, leadingSegmentCount),
      "…",
      ...segments.slice(-trailingSegmentCount)
    ].join(separator)}`;
    const candidate = hasTrailingSeparator ? `${shortenedPath}${separator}` : shortenedPath;

    if (characterCount(candidate) <= normalizedMaxLength) return candidate;
  }

  return truncateCharactersMiddle(path, normalizedMaxLength);
}
