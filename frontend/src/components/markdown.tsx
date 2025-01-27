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
import { Check, Copy } from "lucide-react";

async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (error) {
    console.error("Failed to copy text: ", error);
  }
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

function escapeBrackets(text: string) {
  const pattern = /(```[\s\S]*?```|`.*?`)|\\\[([\s\S]*?[^\\])\\\]|\\\((.*?)\\\)/g;
  return text.replace(pattern, (match, codeBlock, squareBracket, roundBracket) => {
    if (codeBlock) {
      return codeBlock;
    } else if (squareBracket) {
      return `$$${squareBracket}$$`;
    } else if (roundBracket) {
      return `$${roundBracket}$`;
    }
    return match;
  });
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

export function Markdown(
  props: {
    content: string;
    loading?: boolean;
    fontSize?: number;
    fontFamily?: string;
    parentRef?: RefObject<HTMLDivElement>;
    defaultShow?: boolean;
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
      {props.loading ? (
        <>
          <MarkdownContent content={props.content} />
          <div className="italic text-muted-foreground animate-pulse">
            Thinking and encrypting...
          </div>
        </>
      ) : (
        <MarkdownContent content={props.content} />
      )}
    </div>
  );
}
