import type React from "react";

export interface ChatComposerListEdit {
  value: string;
  selectionStart: number;
  selectionEnd: number;
}

type ChatComposerSelectionTarget = Pick<
  HTMLTextAreaElement,
  "scrollHeight" | "scrollTop" | "setSelectionRange"
>;

export function restoreChatComposerListSelection(
  textarea: ChatComposerSelectionTarget,
  edit: ChatComposerListEdit,
  shouldScrollToEnd: boolean
): void {
  textarea.setSelectionRange(edit.selectionStart, edit.selectionEnd);
  if (shouldScrollToEnd) {
    textarea.scrollTop = textarea.scrollHeight;
  }
}

interface ListMarkerMatch {
  indentation: string;
  continuationPrefix: string;
  itemText: string;
}

function listMarkerForLine(line: string): ListMarkerMatch | null {
  const unordered = line.match(/^([ \t]*)([-+*])([ \t]+)(.*)$/);
  if (unordered) {
    const [, indentation, marker, spacing, itemText] = unordered;
    const task = itemText.match(/^\[([ xX])\]([ \t]*)(.*)$/);
    if (task && (task[2] || !task[3])) {
      return {
        indentation,
        continuationPrefix: `${indentation}${marker}${spacing}[ ]${task[2]}`,
        itemText: task[3]
      };
    }

    return {
      indentation,
      continuationPrefix: `${indentation}${marker}${spacing}`,
      itemText
    };
  }

  const ordered = line.match(/^([ \t]*)(\d+)([.)])([ \t]+)(.*)$/);
  if (!ordered) return null;

  return {
    indentation: ordered[1],
    continuationPrefix: `${ordered[1]}${BigInt(ordered[2]) + 1n}${ordered[3]}${ordered[4]}`,
    itemText: ordered[5]
  };
}

function isInsideFence(value: string, lineStart: number): boolean {
  let fence: { marker: "`" | "~"; length: number } | null = null;

  for (const line of value.slice(0, lineStart).split("\n")) {
    if (fence) {
      const closing = line.match(/^[ ]{0,3}(`+|~+)[ \t]*$/);
      if (closing && closing[1][0] === fence.marker && closing[1].length >= fence.length) {
        fence = null;
      }
      continue;
    }

    const opening = line.match(/^[ ]{0,3}(`{3,}|~{3,})/);
    if (opening) {
      fence = {
        marker: opening[1][0] as "`" | "~",
        length: opening[1].length
      };
    }
  }

  return fence !== null;
}

function isHorizontalRule(line: string): boolean {
  const trimmed = line.trim();
  return /^(?:-\s*){3,}$/.test(trimmed) || /^(?:\*\s*){3,}$/.test(trimmed);
}

export function chatComposerListEdit(
  value: string,
  selectionStart: number,
  selectionEnd: number
): ChatComposerListEdit | null {
  const lineStart = value.lastIndexOf("\n", selectionStart - 1) + 1;
  if (isInsideFence(value, lineStart)) return null;

  const lineBreak = value.indexOf("\n", selectionStart);
  const lineEnd = lineBreak === -1 ? value.length : lineBreak;
  const line = value.slice(lineStart, lineEnd);
  if (isHorizontalRule(line)) return null;

  const lineBeforeSelection = value.slice(lineStart, selectionStart);
  const match = listMarkerForLine(lineBeforeSelection);
  if (!match) return null;

  const fullLineMatch = listMarkerForLine(line);
  if (fullLineMatch && fullLineMatch.itemText.trim() === "") {
    const caret = lineStart + match.indentation.length;
    return {
      value: `${value.slice(0, caret)}${value.slice(Math.max(lineEnd, selectionEnd))}`,
      selectionStart: caret,
      selectionEnd: caret
    };
  }

  const continuation = `\n${match.continuationPrefix}`;
  const caret = selectionStart + continuation.length;
  return {
    value: `${value.slice(0, selectionStart)}${continuation}${value.slice(selectionEnd)}`,
    selectionStart: caret,
    selectionEnd: caret
  };
}

function applyChatComposerListEdit(
  textarea: HTMLTextAreaElement,
  edit: ChatComposerListEdit,
  onInputChange: (value: string) => void
): void {
  const originalValue = textarea.value;
  const shouldScrollToEnd = textarea.selectionEnd === originalValue.length;
  let replaceStart = 0;
  while (
    replaceStart < originalValue.length &&
    replaceStart < edit.value.length &&
    originalValue[replaceStart] === edit.value[replaceStart]
  ) {
    replaceStart += 1;
  }

  let originalEnd = originalValue.length;
  let editEnd = edit.value.length;
  while (
    originalEnd > replaceStart &&
    editEnd > replaceStart &&
    originalValue[originalEnd - 1] === edit.value[editEnd - 1]
  ) {
    originalEnd -= 1;
    editEnd -= 1;
  }

  textarea.setSelectionRange(replaceStart, originalEnd);
  const replacement = edit.value.slice(replaceStart, editEnd);
  const appliedWithUndo = document.execCommand?.("insertText", false, replacement) ?? false;
  if (!appliedWithUndo) {
    textarea.setRangeText(replacement, replaceStart, originalEnd, "end");
  }
  onInputChange(edit.value);

  requestAnimationFrame(() => {
    if (textarea.isConnected && document.activeElement === textarea) {
      restoreChatComposerListSelection(textarea, edit, shouldScrollToEnd);
    }
  });
}

function continueListForTextarea(
  event: React.SyntheticEvent<HTMLTextAreaElement>,
  onInputChange: (value: string) => void
): boolean {
  const textarea = event.currentTarget;
  const edit = chatComposerListEdit(textarea.value, textarea.selectionStart, textarea.selectionEnd);
  if (!edit) return false;

  event.preventDefault();
  applyChatComposerListEdit(textarea, edit, onInputChange);
  return true;
}

export function continueChatComposerList(
  event: React.KeyboardEvent<HTMLTextAreaElement>,
  onInputChange: (value: string) => void
): boolean {
  if (event.key !== "Enter" || event.nativeEvent.isComposing) return false;
  return continueListForTextarea(event, onInputChange);
}

export function continueChatComposerListBeforeInput(
  event: React.FormEvent<HTMLTextAreaElement>,
  onInputChange: (value: string) => void
): boolean {
  const inputEvent = event.nativeEvent as InputEvent;
  if (
    inputEvent.isComposing ||
    (inputEvent.inputType !== "insertLineBreak" && inputEvent.inputType !== "insertParagraph")
  ) {
    return false;
  }

  return continueListForTextarea(event, onInputChange);
}
