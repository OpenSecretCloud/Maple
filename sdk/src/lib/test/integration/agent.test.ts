import { expect, test } from "bun:test";
import { createCustomFetch } from "../../ai";
import {
  createSubagent,
  deleteMainAgent,
  deleteSubagent,
  fetchLogin,
  fetchSignUp,
  getMainAgent,
  getMainAgentItem,
  getSubagent,
  getSubagentItem,
  listMainAgentItems,
  listSubagentItems,
  listSubagents
} from "../../api";

const TEST_EMAIL = process.env.VITE_TEST_EMAIL;
const TEST_PASSWORD = process.env.VITE_TEST_PASSWORD;
const TEST_CLIENT_ID = process.env.VITE_TEST_CLIENT_ID;
const API_URL = process.env.VITE_OPEN_SECRET_API_URL;

if (!TEST_EMAIL || !TEST_PASSWORD || !TEST_CLIENT_ID || !API_URL) {
  throw new Error("Test credentials must be set in .env.local");
}

async function setupTestUser() {
  try {
    const { access_token, refresh_token } = await fetchLogin(
      TEST_EMAIL!,
      TEST_PASSWORD!,
      TEST_CLIENT_ID!
    );
    window.localStorage.setItem("access_token", access_token);
    window.localStorage.setItem("refresh_token", refresh_token);
  } catch {
    console.log("Login failed, attempting signup");
    await fetchSignUp(TEST_EMAIL!, TEST_PASSWORD!, "", TEST_CLIENT_ID!, "Test User");
    const { access_token, refresh_token } = await fetchLogin(
      TEST_EMAIL!,
      TEST_PASSWORD!,
      TEST_CLIENT_ID!
    );
    window.localStorage.setItem("access_token", access_token);
    window.localStorage.setItem("refresh_token", refresh_token);
  }
}

async function createTestSubagent() {
  const suffix = Date.now();

  return createSubagent({
    display_name: `SDK Test ${suffix}`,
    purpose: `TypeScript SDK integration test subagent ${suffix}`
  });
}

async function readAgentStream(url: string, input: string): Promise<string> {
  const customFetch = createCustomFetch();

  const response = await customFetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream"
    },
    body: JSON.stringify({ input })
  });

  expect(response.ok).toBe(true);

  const text = await response.text();
  expect(text.length).toBeGreaterThan(0);

  const hasAgentEvents =
    text.includes("agent.message") || text.includes("agent.done") || text.includes("messages");
  expect(hasAgentEvents).toBe(true);

  return text;
}

test.skip("Create and delete subagent", async () => {
  await setupTestUser();

  const subagent = await createTestSubagent();

  expect(subagent.id).toBeDefined();
  expect(subagent.object).toBe("agent.subagent");
  expect(subagent.conversation_id).toBeDefined();
  expect(subagent.display_name.length).toBeGreaterThan(0);
  expect(subagent.purpose.length).toBeGreaterThan(0);

  const deleted = await deleteSubagent(subagent.id);

  expect(deleted.deleted).toBe(true);
  expect(deleted.id).toBe(subagent.id);
  expect(deleted.object).toBe("agent.subagent.deleted");
});

test.skip("Get main agent and list main agent items", async () => {
  await setupTestUser();

  const mainAgent = await getMainAgent();

  expect(mainAgent.id).toBeDefined();
  expect(mainAgent.object).toBe("agent.main");
  expect(mainAgent.kind).toBe("main");
  expect(mainAgent.conversation_id).toBeDefined();
  expect(mainAgent.display_name.length).toBeGreaterThan(0);

  const items = await listMainAgentItems({ limit: 10, order: "desc" });

  expect(items.object).toBe("list");
  expect(Array.isArray(items.data)).toBe(true);

  if (items.data.length > 0) {
    const item = await getMainAgentItem(items.data[0].id);
    expect(item.id).toBe(items.data[0].id);
  }
});

test.skip("Delete main agent resets the agent tree", async () => {
  await setupTestUser();

  const mainAgent = await getMainAgent();
  const subagent = await createTestSubagent();

  const deleted = await deleteMainAgent();

  expect(deleted.deleted).toBe(true);
  expect(deleted.id).toBe(mainAgent.id);
  expect(deleted.object).toBe("agent.main.deleted");

  const subagents = await listSubagents({ limit: 10 });
  expect(subagents.data.some((item) => item.id === subagent.id)).toBe(false);

  const recreatedMainAgent = await getMainAgent();
  expect(recreatedMainAgent.id).toBeDefined();
});

test.skip("List and get subagents", async () => {
  await setupTestUser();

  const subagent = await createTestSubagent();

  try {
    const list = await listSubagents({ limit: 10, created_by: "user" });

    expect(list.object).toBe("list");
    expect(Array.isArray(list.data)).toBe(true);
    expect(list.data.some((item) => item.id === subagent.id)).toBe(true);

    const fetched = await getSubagent(subagent.id);

    expect(fetched.id).toBe(subagent.id);
    expect(fetched.object).toBe("agent.subagent");
    expect(fetched.kind).toBe("subagent");
  } finally {
    await deleteSubagent(subagent.id);
  }
});

test.skip("Agent chat via SSE (using createCustomFetch)", async () => {
  await setupTestUser();

  const text = await readAgentStream(
    `${API_URL}/v1/agent/chat`,
    "Hello, please respond with just the word 'pong'."
  );

  console.log("Agent chat SSE completed, response length:", text.length);
}, 60000);

test.skip("Subagent chat via SSE (using createCustomFetch)", async () => {
  await setupTestUser();

  const subagent = await createTestSubagent();

  try {
    const text = await readAgentStream(
      `${API_URL}/v1/agent/subagents/${encodeURIComponent(subagent.id)}/chat`,
      "Please reply with the word 'subpong'."
    );

    const items = await listSubagentItems(subagent.id, { limit: 10, order: "desc" });

    expect(items.object).toBe("list");
    expect(Array.isArray(items.data)).toBe(true);

    if (items.data.length > 0) {
      const item = await getSubagentItem(subagent.id, items.data[0].id);
      expect(item.id).toBe(items.data[0].id);
    }

    console.log("Subagent chat SSE completed, response length:", text.length);
  } finally {
    await deleteSubagent(subagent.id);
  }
}, 60000);
