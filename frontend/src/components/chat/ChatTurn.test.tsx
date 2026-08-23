import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatAssistantTurn, ChatUserTurn } from "./ChatTurn";

describe("shared chat typography scope", () => {
  test("applies the typography scope to user turns", () => {
    const markup = renderToStaticMarkup(
      <ChatUserTurn>
        <p>User message</p>
      </ChatUserTurn>
    );

    expect(markup).toContain("chat-typography");
    expect(markup).toContain("User message");
    expect(markup).toContain("pt-4");
    expect(markup).toContain("pb-4");
    expect(markup).not.toContain("data-stacked-user");
  });

  test("keeps actions below on mobile and beside compact consecutive turns on desktop", () => {
    const markup = renderToStaticMarkup(
      <ChatUserTurn stackedTop stackedBottom actions={<button type="button">Copy</button>}>
        <p>Follow-up</p>
      </ChatUserTurn>
    );

    expect(markup).toContain("data-stacked-user");
    expect(markup).toContain("pt-1 -mt-1");
    expect(markup).toContain("pb-0");
    expect(markup).toContain("justify-end pr-1 pt-1");
    expect(markup).toContain("md:absolute md:bottom-0 md:right-full");
    expect(markup).toContain("Copy");
  });

  test("applies the typography scope to assistant, reasoning, and tool content", () => {
    const markup = renderToStaticMarkup(
      <ChatAssistantTurn>
        <div className="text-sm">Tool activity</div>
      </ChatAssistantTurn>
    );

    expect(markup).toContain("chat-typography");
    expect(markup).toContain("Tool activity");
  });
});
