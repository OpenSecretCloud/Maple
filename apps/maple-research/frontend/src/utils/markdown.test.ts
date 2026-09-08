import { describe, expect, it } from "bun:test";

import { truncateMarkdownPreservingLinks } from "./markdown";

describe("truncateMarkdownPreservingLinks", () => {
  it("returns the original text when already within maxLength", () => {
    expect(truncateMarkdownPreservingLinks("hello", 10)).toBe("hello");
  });

  it("safely truncates when there are no links", () => {
    expect(truncateMarkdownPreservingLinks("abcdef", 3)).toBe("abc...");
  });

  it("converts a plain URL into a markdown link when truncation would cut it", () => {
    const before = "Check this ";
    const url = "https://example.com/very/long/path";
    const after = " for more";
    const text = `${before}${url}${after}`;

    const maxLength = before.length + 12;
    expect(maxLength).toBeGreaterThan(before.length);
    expect(maxLength).toBeLessThan(before.length + url.length);

    const result = truncateMarkdownPreservingLinks(text, maxLength);
    expect(result).toBe(`${before}[${url.slice(0, 12)}...](${url})`);
  });

  it("does not convert a URL that is already inside a markdown link", () => {
    const before = "Go to ";
    const link = "[site](https://example.com/very/long/path)";
    const after = " now";
    const text = `${before}${link}${after}`;

    const maxLength = before.length + 5;
    const result = truncateMarkdownPreservingLinks(text, maxLength);

    expect(result).toBe("Go to...");
  });

  it("truncates before a markdown link if truncation would break it", () => {
    const before = "Start ";
    const link = "[docs](https://example.com/docs)";
    const after = " end";
    const text = `${before}${link}${after}`;

    const maxLength = before.length + 1;
    const result = truncateMarkdownPreservingLinks(text, maxLength);

    expect(result).toBe("Start...");
  });

  it("does not convert a URL when truncation happens exactly at the URL boundary", () => {
    const before = "Before ";
    const url = "https://example.com/very/long/path";
    const after = " after";
    const text = `${before}${url}${after}`;

    expect(truncateMarkdownPreservingLinks(text, before.length)).toBe(`${before}...`);
    expect(truncateMarkdownPreservingLinks(text, before.length + url.length)).toBe(
      `${before}${url}...`
    );
  });

  it("converts the correct plain URL when multiple links exist", () => {
    const md = "See [x](https://x.test) and ";
    const url = "https://example.com/very/long/path";
    const after = " for details";
    const text = `${md}${url}${after}`;

    const maxLength = md.length + 8;
    const result = truncateMarkdownPreservingLinks(text, maxLength);

    expect(result).toBe(`${md}[${url.slice(0, 8)}...](${url})`);
  });
});
