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
