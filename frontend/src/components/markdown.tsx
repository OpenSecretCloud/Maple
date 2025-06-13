// Mostly borrowed from https://github.com/ChatGPTNextWeb/ChatGPT-Next-Web/blob/b6bb1673d469a05ddda97a27730e51ae7a231187/app/components/markdown.tsx#L281
import ReactMarkdown from "react-markdown";
import "katex/dist/katex.min.css";
import RemarkMath from "remark-math";
import RemarkBreaks from "remark-breaks";
import RehypeKatex from "rehype-katex";
import RemarkGfm from "remark-gfm";
import RehypeHighlight from "rehype-highlight";
import { useRef, useState, RefObject, useEffect, useMemo } from "react";
import React from "react";
import { Button } from "./ui/button";
import { Check, Copy, ChevronDown, ChevronRight, Brain } from "lucide-react";

async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (error) {
    console.error("Failed to copy text: ", error);
  }
}

interface ThinkingBlockProps {
  content: string;
  isThinking: boolean;
  duration?: number;
}

function ThinkingBlock({ content, isThinking, duration }: ThinkingBlockProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const startTimeRef = useRef<number | null>(null);

  // Update elapsed time every second while thinking
  useEffect(() => {
    if (!isThinking) {
      return;
    }

    // Set start time when thinking begins
    if (startTimeRef.current === null) {
      startTimeRef.current = Date.now();
    }

    // Start counting immediately
    const elapsed = Math.floor((Date.now() - startTimeRef.current) / 1000);
    setElapsedSeconds(elapsed);

    const interval = setInterval(() => {
      if (startTimeRef.current !== null) {
        const elapsed = Math.floor((Date.now() - startTimeRef.current) / 1000);
        setElapsedSeconds(elapsed);
      }
    }, 1000);

    return () => {
      clearInterval(interval);
      startTimeRef.current = null;
    };
  }, [isThinking]);

  // Calculate duration text - use actual duration, elapsed time, or estimate based on word count
  const displayDuration = useMemo(() => {
    if (duration) return duration;
    if (isThinking) return elapsedSeconds;

    // Fallback: estimate based on word count
    const wordCount = content.trim().split(/\s+/).length;
    // Slower estimation to better match observed times (26 actual vs 23 estimated)
    const estimatedSeconds = Math.max(1, Math.round(wordCount / 26)); // ~26 words per second
    return estimatedSeconds;
  }, [content, duration, isThinking, elapsedSeconds]);

  const durationText = `${displayDuration}`;

  return (
    <div className="my-3 border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden bg-gray-50 dark:bg-gray-900/50">
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full px-4 py-2 flex items-center gap-2 hover:bg-gray-100 dark:hover:bg-gray-800/50 transition-colors text-left"
        aria-expanded={isExpanded}
      >
        <span className="text-gray-500 dark:text-gray-400">
          {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        </span>
        <Brain className="h-4 w-4 text-gray-500 dark:text-gray-400" />
        <span className="text-sm text-gray-600 dark:text-gray-300 font-medium">
          {isThinking ? (
            <span className="flex items-center gap-2">
              Thinking for {durationText} seconds
              <span className="flex gap-1">
                <span className="animate-bounce" style={{ animationDelay: "0ms" }}>
                  .
                </span>
                <span className="animate-bounce" style={{ animationDelay: "150ms" }}>
                  .
                </span>
                <span className="animate-bounce" style={{ animationDelay: "300ms" }}>
                  .
                </span>
              </span>
            </span>
          ) : (
            `Thought for ${durationText} seconds`
          )}
        </span>
      </button>
      {isExpanded && (
        <div className="px-4 py-3 border-t border-gray-200 dark:border-gray-700">
          <div className="text-sm text-gray-600 dark:text-gray-400 whitespace-pre-wrap">
            {content}
          </div>
        </div>
      )}
    </div>
  );
}

interface ParsedContent {
  type: "thinking" | "content";
  content: string;
  duration?: string; // Store duration as string like "23"
  id?: string; // Unique ID to track thinking blocks across renders
}

function parseThinkingTags(content: string, isComplete: boolean = false): ParsedContent[] {
  const parts: ParsedContent[] = [];

  // Check for the edge case: <think> followed by only whitespace and no closing tag
  if (isComplete && /^<think>\s*$/m.test(content) && !content.includes("</think>")) {
    // Strip out the incomplete <think> tag and treat everything after as content
    const strippedContent = content.replace(/^<think>\s*/m, "");
    if (strippedContent.trim()) {
      parts.push({ type: "content", content: strippedContent });
    }
    return parts;
  }

  // Pattern to match <think> tags (complete or incomplete)
  // During streaming (!isComplete), we want to catch <think> as soon as it appears
  const thinkPattern = isComplete
    ? /<think>([\s\S]*?)<\/think>|<think>([\s\S]*?)$/g
    : /<think>([\s\S]*?)(?:<\/think>|$)/g;

  let lastIndex = 0;
  let match;

  while ((match = thinkPattern.exec(content)) !== null) {
    // Add content before the think tag
    if (match.index > lastIndex) {
      const beforeContent = content.slice(lastIndex, match.index);
      if (beforeContent.trim()) {
        parts.push({ type: "content", content: beforeContent });
      }
    }

    // Extract content from the match
    const thinkContent = match[1] ?? match[2] ?? "";

    // During streaming, even empty think tags should be shown to indicate thinking is starting
    if (!isComplete && match[0].includes("<think>")) {
      parts.push({
        type: "thinking",
        content: thinkContent,
        duration: undefined,
        id: `think-${match.index}`
      });
    } else if (thinkContent.trim()) {
      // For complete content, only add if there's actual content
      parts.push({
        type: "thinking",
        content: thinkContent,
        duration: undefined,
        id: `think-${match.index}`
      });
    }

    lastIndex = match.index + match[0].length;
  }

  // Add any remaining content
  if (lastIndex < content.length) {
    const remainingContent = content.slice(lastIndex);
    if (remainingContent.trim()) {
      parts.push({ type: "content", content: remainingContent });
    }
  }

  return parts;
}

export function stripThinkingTags(content: string): string {
  return (
    parseThinkingTags(content, true) // leverage single source of truth
      .filter((p) => p.type === "content")
      .map((p) => p.content)
      .join("")
      // collapse ≥3 consecutive blank lines to two
      .replace(/\n{3,}/g, "\n\n")
      .trim()
  );
}

export function PreCode(props: JSX.IntrinsicElements["pre"]) {
  const ref = useRef<HTMLPreElement>(null);

  const [isCopied, setIsCopied] = useState(false);

  //Wrap the paragraph for plain-text
  useEffect(() => {
    if (ref.current) {
      const codeElements = ref.current.querySelectorAll("code") as NodeListOf<HTMLElement>;
      const wrapLanguages = ["", "md", "markdown", "text", "txt", "plaintext", "tex", "latex"];
      codeElements.forEach((codeElement) => {
        const languageClass = codeElement.className.match(/language-(\w+)/);
        const name = languageClass ? languageClass[1] : "";
        if (wrapLanguages.includes(name)) {
          codeElement.style.whiteSpace = "pre-wrap";
        }
      });
    }
  }, []);

  return (
    <>
      <div className="flex justify-end pt-2 mb-1">
        <Button
          variant="ghost"
          size="sm"
          className="gap-2"
          onClick={() => {
            if (ref.current) {
              const code = ref.current.innerText;
              copyToClipboard(code);
              setIsCopied(true);
              setTimeout(() => setIsCopied(false), 2000);
            }
          }}
          aria-label={isCopied ? "Copied" : "Copy to clipboard"}
        >
          {isCopied ? (
            <>
              <Check className="h-4 w-4" />
              <span className="font-sans text-sm">Copied</span>
            </>
          ) : (
            <>
              <Copy className="h-4 w-4" />
              <span className="font-sans text-sm">Copy Code</span>
            </>
          )}
        </Button>
      </div>
      <pre ref={ref}>{props.children}</pre>
    </>
  );
}

function CustomCode(props: JSX.IntrinsicElements["code"]) {
  return <code>{props.children}</code>;
}

function ResponsiveTable({ children, className, ...rest }: JSX.IntrinsicElements["table"]) {
  // Strip off props added by react-markdown that the DOM doesn't understand
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const { node, inline, ...safeRest } = rest as Record<string, unknown>;

  return (
    <div className="overflow-x-auto max-w-full w-0 min-w-full rounded-md border border-border/50">
      <table className={className} {...(safeRest as object)}>
        {children}
      </table>
    </div>
  );
}

function escapeDollarNumber(text: string) {
  let escapedText = "";

  for (let i = 0; i < text.length; i += 1) {
    let char = text[i];
    const nextChar = text[i + 1] || " ";

    if (char === "$" && nextChar >= "0" && nextChar <= "9") {
      char = "\\$";
    }

    escapedText += char;
  }

  return escapedText;
}

function escapeBrackets(text: string): string {
  // First, handle code blocks to protect them from processing
  const codeBlockPattern = /```[\s\S]*?```|`.*?`/g;
  const codeBlocks: string[] = [];
  const textWithoutCode = text.replace(codeBlockPattern, function (match: string) {
    codeBlocks.push(match);
    return `__CODE_BLOCK_${codeBlocks.length - 1}__`;
  });

  // Process escaped square brackets - LaTeX display mode
  const squareBracketPattern = /\\\[([\s\S]*?[^\\])\\\]/g;
  let result = textWithoutCode.replace(
    squareBracketPattern,
    function (_match: string, content: string) {
      return `$$${content}$$`;
    }
  );

  // Process escaped parentheses - LaTeX inline mode
  const roundBracketPattern = /\\\((.*?)\\\)/g;
  result = result.replace(roundBracketPattern, function (_match: string, content: string) {
    return `$${content}$`;
  });

  // Restore code blocks
  codeBlocks.forEach(function (block: string, i: number) {
    result = result.replace(`__CODE_BLOCK_${i}__`, block);
  });

  return result;
}

function MarkDownContentToMemo(props: { content: string }) {
  const escapedContent = useMemo(() => {
    return escapeBrackets(escapeDollarNumber(props.content));
  }, [props.content]);

  return (
    <ReactMarkdown
      remarkPlugins={[RemarkMath, RemarkGfm, RemarkBreaks]}
      rehypePlugins={[
        RehypeKatex,
        [
          RehypeHighlight,
          {
            detect: false,
            ignoreMissing: true
          }
        ]
      ]}
      components={{
        pre: (props: JSX.IntrinsicElements["pre"]) => <PreCode {...props} />,
        code: (props: JSX.IntrinsicElements["code"]) => <CustomCode {...props} />,
        table: (props: JSX.IntrinsicElements["table"]) => <ResponsiveTable {...props} />,
        p: (pProps) => <p {...pProps} dir="auto" />,
        a: (aProps) => {
          const href = aProps.href || "";
          const isInternal = /^\/#/i.test(href);
          const target = isInternal ? "_self" : (aProps.target ?? "_blank");
          return <a {...aProps} target={target} />;
        }
      }}
    >
      {escapedContent}
    </ReactMarkdown>
  );
}

export const MarkdownContent = React.memo(MarkDownContentToMemo);

function MarkdownWithThinking({
  content,
  loading = false,
  chatId
}: {
  content: string;
  loading?: boolean;
  chatId?: string;
}) {
  const parsedContent = useMemo(() => {
    // Pass isComplete (which is !loading) to handle the edge case
    return parseThinkingTags(content, !loading);
  }, [content, loading]);

  return (
    <>
      {parsedContent.map((part, index) => {
        if (part.type === "thinking") {
          // Check if this thinking block is still being streamed
          const isLastPart = index === parsedContent.length - 1;
          // During streaming, check if this thinking block doesn't have a closing tag
          const thisThinkingPosition = content.lastIndexOf("<think>");
          const closingPosition = content.lastIndexOf("</think>");

          // It's actively thinking if we're loading and this think tag hasn't been closed yet
          const isThinking = loading && isLastPart && closingPosition < thisThinkingPosition;

          return (
            <ThinkingBlock
              key={`${chatId}-${part.id || index}`} // Include chatId in key to reset state
              content={part.content}
              isThinking={isThinking}
              duration={undefined} // Let ThinkingBlock handle duration internally
            />
          );
        } else {
          return <MarkdownContent key={index} content={part.content} />;
        }
      })}
    </>
  );
}

export function Markdown(
  props: {
    content: string;
    loading?: boolean;
    fontSize?: number;
    fontFamily?: string;
    parentRef?: RefObject<HTMLDivElement>;
    defaultShow?: boolean;
    chatId?: string;
  } & React.DOMAttributes<HTMLDivElement>
) {
  const mdRef = useRef<HTMLDivElement>(null);

  return (
    <div
      className="markdown-body"
      style={{
        fontSize: `${props.fontSize ?? 16}px`,
        fontFamily: props.fontFamily || "inherit"
      }}
      ref={mdRef}
      onContextMenu={props.onContextMenu}
      onDoubleClickCapture={props.onDoubleClickCapture}
      dir="auto"
    >
      <MarkdownWithThinking content={props.content} loading={props.loading} chatId={props.chatId} />
      {props.loading && !props.content.trim() && (
        <div className="italic text-muted-foreground animate-pulse">Thinking and encrypting...</div>
      )}
    </div>
  );
}
