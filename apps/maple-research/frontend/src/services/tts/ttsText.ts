export const TTS_CHUNK_MAX_WORDS = 300;
export const TTS_CHUNK_MAX_UNBROKEN_CHARACTERS = 1_000;

const ENDING_PUNCTUATION = /[.!?;:,'"\u201c\u201d\u2018\u2019)\]}…。」』】〉》›»]$/;
const EMOJI =
  /[\u{1f600}-\u{1f64f}\u{1f300}-\u{1f5ff}\u{1f680}-\u{1f6ff}\u{1f700}-\u{1f77f}\u{1f780}-\u{1f7ff}\u{1f800}-\u{1f8ff}\u{1f900}-\u{1f9ff}\u{1fa00}-\u{1fa6f}\u{1fa70}-\u{1faff}\u{2600}-\u{26ff}\u{2700}-\u{27bf}\u{1f1e6}-\u{1f1ff}]+/gu;
const EMOJI_JOINERS_AND_VARIANTS = /\u200d|\u20e3|\ufe0e|\ufe0f|[\u{e0020}-\u{e007f}]/gu;
const UNORDERED_LIST_MARKER =
  "[-+*\u2022\u2023\u2043\u2219\u00b7\u25e6\u25aa\u25ab\u25cf\u25cb\u25a0\u25a1]";
const ORDERED_LIST_MARKER = "(?:\\d{1,4}[.)]|\\(\\d{1,4}\\))";
const DASH_SEPARATOR = /[ \t]+[-\u2011\u2012][ \t]+|[ \t]*[\u2013-\u2015][ \t]*/g;
const PAUSE_PUNCTUATION = /[,.!?;:]$/;
const LIST_ITEM_PAUSE_PUNCTUATION = /[,.!?;:…]$/;
const RANGE_TOKEN_CHARACTER = /[\p{L}\p{N}]/u;

function wordsIn(value: string): string[] {
  const trimmed = value.trim();
  return trimmed ? trimmed.split(/\s+/) : [];
}

function wordCount(value: string): number {
  return wordsIn(value).length;
}

function characterCount(value: string): number {
  return Array.from(value).length;
}

function replaceLiteral(text: string, from: string, to: string): string {
  return text.split(from).join(to);
}

function stripFencedCodeBlocks(text: string): string {
  const lines = text.split(/\r?\n/);
  const output: string[] = [];
  let inFence = false;
  let fenceCharacter: "`" | "~" | null = null;
  let fenceLength = 0;

  for (const line of lines) {
    if (!inFence) {
      const openMatch = line.match(/^\s*(?:>+\s*)?([`~]{3,})[^\n]*$/);
      if (openMatch) {
        inFence = true;
        fenceCharacter = openMatch[1][0] as "`" | "~";
        fenceLength = openMatch[1].length;
        continue;
      }

      output.push(line);
      continue;
    }

    const closeMatch = line.match(/^\s*(?:>+\s*)?([`~]{3,})\s*$/);
    if (!closeMatch) {
      continue;
    }

    const fence = closeMatch[1];
    if (fenceCharacter && fence[0] === fenceCharacter && fence.length >= fenceLength) {
      inFence = false;
      fenceCharacter = null;
      fenceLength = 0;
    }
  }

  return output.join("\n");
}

function isHorizontalRule(line: string): boolean {
  const compact = line.trim().replace(/[ \t]/g, "");
  return /^(?:-{3,}|\*{3,}|_{3,})$/.test(compact);
}

function replaceDashSeparators(line: string): string {
  return line.replace(DASH_SEPARATOR, (match, offset: number, source: string) => {
    const before = source.slice(0, offset).trimEnd();
    const after = source.slice(offset + match.length).trimStart();

    if (
      match === "–" &&
      RANGE_TOKEN_CHARACTER.test(before.at(-1) ?? "") &&
      RANGE_TOKEN_CHARACTER.test(after.at(0) ?? "")
    ) {
      return " to ";
    }

    if (!before || !after || PAUSE_PUNCTUATION.test(before) || /^[,.!?;:]/.test(after)) {
      return " ";
    }

    return ", ";
  });
}

function addListItemPause(line: string): string {
  const trimmed = line.trimEnd();
  const withoutTrailingMarkdown = trimmed.replace(/[*_~`]+$/, "").trimEnd();

  if (!trimmed || LIST_ITEM_PAUSE_PUNCTUATION.test(withoutTrailingMarkdown)) {
    return trimmed;
  }

  return `${trimmed}.`;
}

function sanitizeMarkdownLineStructure(text: string): string {
  const listMarker = new RegExp(
    `^[ \\t]*(?:${UNORDERED_LIST_MARKER}|${ORDERED_LIST_MARKER})[ \\t]+`
  );

  return text
    .split(/\r?\n/)
    .map((line) => {
      if (isHorizontalRule(line)) {
        return "";
      }

      const listItem = listMarker.test(line);
      const sanitizedLine = replaceDashSeparators(line.replace(listMarker, ""));
      return listItem ? addListItemPause(sanitizedLine) : sanitizedLine;
    })
    .join("\n");
}

/**
 * Removes content and formatting that should not be spoken while preserving
 * paragraph boundaries for the chunk planner.
 */
export function sanitizeTextForTTS(text: string): string {
  let sanitized = stripFencedCodeBlocks(text);

  sanitized = sanitized.replace(/<think>[\s\S]*?<\/think>/gi, "");
  sanitized = sanitized.replace(/<think>[\s\S]*$/gi, "");
  sanitized = sanitized.normalize("NFC");
  sanitized = sanitizeMarkdownLineStructure(sanitized);

  sanitized = sanitized.replace(/\*\*([^*]+)\*\*/g, "$1");
  sanitized = sanitized.replace(/__([^_]+)__/g, "$1");
  sanitized = sanitized.replace(/\*([^*]+)\*/g, "$1");
  sanitized = sanitized.replace(/_([^_\s][^_]*)_/g, "$1");
  sanitized = sanitized.replace(/~~([^~]+)~~/g, "$1");
  sanitized = sanitized.replace(/`([^`]+)`/g, "$1");
  sanitized = sanitized.replace(/^\s*#{1,6}\s*/gm, "");
  sanitized = sanitized.replace(/<\/?[A-Za-z][A-Za-z0-9_-]*(?:\s+[^>]*)?>/g, " ");
  sanitized = sanitized.replace(EMOJI, "");
  sanitized = sanitized.replace(EMOJI_JOINERS_AND_VARIANTS, "");

  const replacements: ReadonlyArray<readonly [string, string]> = [
    ["¯", " "],
    ["_", " "],
    ["\u201c", '"'],
    ["\u201d", '"'],
    ["\u2018", "'"],
    ["\u2019", "'"],
    ["´", "'"],
    ["`", "'"],
    ["[", " "],
    ["]", " "],
    ["|", " "],
    ["/", " "],
    ["#", " "],
    ["→", " "],
    ["←", " "]
  ];
  for (const [from, to] of replacements) {
    sanitized = replaceLiteral(sanitized, from, to);
  }

  for (const symbol of ["♥", "☆", "♡", "©", "\\"]) {
    sanitized = replaceLiteral(sanitized, symbol, "");
  }

  sanitized = replaceLiteral(sanitized, "@", " at ");
  sanitized = replaceLiteral(sanitized, "e.g.,", "for example, ");
  sanitized = replaceLiteral(sanitized, "i.e.,", "that is, ");

  sanitized = sanitized.replace(/\s+([,.!?;:'])/g, "$1");
  sanitized = sanitized.replace(/"{2,}/g, '"');
  sanitized = sanitized.replace(/'{2,}/g, "'");

  return sanitized
    .split(/\r?\n/)
    .map((line) => line.replace(/[^\S\r\n]+/g, " ").trim())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function normalizeChunk(text: string): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (!normalized || ENDING_PUNCTUATION.test(normalized)) {
    return normalized;
  }
  return `${normalized}.`;
}

function hardSplitCharacters(text: string): string[] {
  const characters = Array.from(text);
  const chunks: string[] = [];
  for (let index = 0; index < characters.length; index += TTS_CHUNK_MAX_UNBROKEN_CHARACTERS) {
    chunks.push(characters.slice(index, index + TTS_CHUNK_MAX_UNBROKEN_CHARACTERS).join(""));
  }
  return chunks;
}

function splitByWords(text: string, maxWords: number): string[] {
  const words = wordsIn(text);
  const chunks: string[] = [];
  let currentWords: string[] = [];

  const flushCurrentWords = () => {
    if (currentWords.length > 0) {
      chunks.push(currentWords.join(" "));
      currentWords = [];
    }
  };

  for (const word of words) {
    if (characterCount(word) > TTS_CHUNK_MAX_UNBROKEN_CHARACTERS) {
      flushCurrentWords();
      chunks.push(...hardSplitCharacters(word));
      continue;
    }

    currentWords.push(word);
    if (currentWords.length === maxWords) {
      flushCurrentWords();
    }
  }

  flushCurrentWords();
  return chunks;
}

function pushChunkUnit(
  chunks: string[],
  current: { value: string },
  unit: string,
  maxWords: number
) {
  const trimmedUnit = unit.trim();
  if (!trimmedUnit) {
    return;
  }

  const unitWords = wordsIn(trimmedUnit);
  const hasOversizedWord = unitWords.some(
    (word) => characterCount(word) > TTS_CHUNK_MAX_UNBROKEN_CHARACTERS
  );
  if (unitWords.length > maxWords || hasOversizedWord) {
    if (current.value) {
      chunks.push(current.value);
      current.value = "";
    }
    chunks.push(...splitByWords(trimmedUnit, maxWords));
    return;
  }

  if (wordCount(current.value) + unitWords.length > maxWords) {
    chunks.push(current.value);
    current.value = "";
  }
  current.value = current.value ? `${current.value} ${trimmedUnit}` : trimmedUnit;
}

/**
 * Plans provider requests by paragraph, sentence, and then word boundaries.
 * Every returned chunk is nonempty and contains at most maxWords whitespace-
 * delimited words.
 */
export function chunkTextForTTS(text: string, maxWords = TTS_CHUNK_MAX_WORDS): string[] {
  if (!Number.isInteger(maxWords) || maxWords <= 0) {
    return [];
  }

  const sanitized = sanitizeTextForTTS(text);
  if (!sanitized) {
    return [];
  }

  const chunks: string[] = [];
  const current = { value: "" };

  for (const paragraph of sanitized.split(/\n\s*\n/)) {
    const trimmedParagraph = paragraph.trim();
    if (!trimmedParagraph) {
      continue;
    }

    if (wordCount(trimmedParagraph) <= maxWords) {
      pushChunkUnit(chunks, current, trimmedParagraph, maxWords);
      continue;
    }

    let sentenceStart = 0;
    for (const match of trimmedParagraph.matchAll(/[.!?]\s+/g)) {
      const matchIndex = match.index;
      const sentence = trimmedParagraph.slice(sentenceStart, matchIndex + 1);
      sentenceStart = matchIndex + match[0].length;
      pushChunkUnit(chunks, current, sentence, maxWords);
    }
    pushChunkUnit(chunks, current, trimmedParagraph.slice(sentenceStart), maxWords);
  }

  if (current.value) {
    chunks.push(current.value);
  }

  return chunks.map(normalizeChunk).filter(Boolean);
}
