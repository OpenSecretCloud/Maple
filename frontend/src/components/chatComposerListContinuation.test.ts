import { describe, expect, test } from "bun:test";

import {
  chatComposerListEdit,
  restoreChatComposerListSelection
} from "./chatComposerListContinuation";

function editAtEnd(value: string) {
  return chatComposerListEdit(value, value.length, value.length);
}

describe("chatComposerListEdit", () => {
  test.each(["-", "*", "+"])("continues an unordered %s list", (marker) => {
    const value = `${marker} first item`;

    expect(editAtEnd(value)).toEqual({
      value: `${value}\n${marker} `,
      selectionStart: value.length + 3,
      selectionEnd: value.length + 3
    });
  });

  test.each([
    ["- [ ] todo", "- [ ] "],
    ["* [x] done", "* [ ] "],
    ["\t+  [X]\tfinished", "\t+  [ ]\t"]
  ])("continues task list %s with an unchecked item", (value, continuation) => {
    expect(editAtEnd(value)).toEqual({
      value: `${value}\n${continuation}`,
      selectionStart: value.length + continuation.length + 1,
      selectionEnd: value.length + continuation.length + 1
    });
  });

  test.each([
    ["  9) ninth item", "  10) "],
    ["\t99. ninety-ninth", "\t100. "]
  ])("increments ordered list %s and preserves its style", (value, continuation) => {
    expect(editAtEnd(value)).toEqual({
      value: `${value}\n${continuation}`,
      selectionStart: value.length + continuation.length + 1,
      selectionEnd: value.length + continuation.length + 1
    });
  });

  test("continues the current list type in a mixed draft", () => {
    const value = "- first\n\n1. second\n\n* third";
    expect(editAtEnd(value)?.value).toBe(`${value}\n* `);
  });

  test("continues the list at the caret and replaces selected text", () => {
    const value = "Intro\n1. first item selected";
    const selectionStart = value.indexOf(" selected");

    expect(chatComposerListEdit(value, selectionStart, value.length)).toEqual({
      value: "Intro\n1. first item\n2. ",
      selectionStart: selectionStart + 4,
      selectionEnd: selectionStart + 4
    });
  });

  test("replaces a selection spanning later lines", () => {
    const value = "1. first selected\nsecond\nthird";
    const selectionStart = value.indexOf(" selected");
    const selectionEnd = value.indexOf("third");

    expect(chatComposerListEdit(value, selectionStart, selectionEnd)).toEqual({
      value: "1. first\n2. third",
      selectionStart: selectionStart + 4,
      selectionEnd: selectionStart + 4
    });
  });

  test("moves text after the caret to the generated item", () => {
    expect(chatComposerListEdit("- item", 2, 2)).toEqual({
      value: "- \n- item",
      selectionStart: 5,
      selectionEnd: 5
    });
  });

  test.each(["First\n  - ", "First\n\t3)  ", "First\n  * [x]  "])(
    "ends an empty list item %s",
    (value) => {
      const lineStart = value.lastIndexOf("\n") + 1;
      const indentation = value.slice(lineStart).match(/^[ \t]*/)?.[0] ?? "";
      const caret = lineStart + indentation.length;
      expect(editAtEnd(value)).toEqual({
        value: `${value.slice(0, lineStart)}${indentation}`,
        selectionStart: caret,
        selectionEnd: caret
      });
    }
  );

  test("removes an empty marker and the selected following text", () => {
    expect(chatComposerListEdit("- \nselected", 2, 11)).toEqual({
      value: "",
      selectionStart: 0,
      selectionEnd: 0
    });
  });

  test.each(["ordinary text", "prefix - item", "\\- escaped", "-not a list", "- - -", "* * * *"])(
    "does not change non-list input %s",
    (value) => {
      expect(editAtEnd(value)).toBeNull();
    }
  );

  test.each([
    "```\n- code item",
    "~~~ts\n1. code item",
    "before\n````js\n* code item",
    "before\n~~~\n+ code item"
  ])("does not continue a list inside a fence", (value) => {
    expect(editAtEnd(value)).toBeNull();
  });

  test("continues again after the matching fence closes", () => {
    expect(editAtEnd("```\n- code\n```\n- real")?.value).toBe("```\n- code\n```\n- real\n- ");
    expect(editAtEnd("~~~~\n1. code\n~~~\n2. still code")).toBeNull();
  });
});

describe("restoreChatComposerListSelection", () => {
  test("scrolls a capped composer to a continuation inserted at the end", () => {
    const selection: [number, number] = [-1, -1];
    const textarea = {
      scrollHeight: 640,
      scrollTop: 120,
      setSelectionRange(start: number, end: number) {
        selection[0] = start;
        selection[1] = end;
      }
    };

    restoreChatComposerListSelection(
      textarea,
      { value: "- first\n- ", selectionStart: 10, selectionEnd: 10 },
      true
    );

    expect(selection).toEqual([10, 10]);
    expect(textarea.scrollTop).toBe(640);
  });

  test("does not jump to the bottom when editing in the middle of a draft", () => {
    const textarea = {
      scrollHeight: 640,
      scrollTop: 120,
      setSelectionRange() {}
    };

    restoreChatComposerListSelection(
      textarea,
      { value: "- first\n- second", selectionStart: 10, selectionEnd: 10 },
      false
    );

    expect(textarea.scrollTop).toBe(120);
  });
});
