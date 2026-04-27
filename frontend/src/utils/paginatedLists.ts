import {
  listConversationProjects,
  type Conversation,
  type ConversationProjectListItem,
  type ConversationsListParams,
  type OpenSecretContextType
} from "@opensecret/react";

const SIDEBAR_PAGE_SIZE = 20;

type ConversationsClient = Pick<OpenSecretContextType, "listConversations">;

export async function listAllConversations(
  client: ConversationsClient,
  params: Omit<ConversationsListParams, "after"> = {}
): Promise<Conversation[]> {
  const conversations: Conversation[] = [];
  const seenConversationIds = new Set<string>();
  let after: string | undefined;

  while (true) {
    const response = await client.listConversations({
      ...params,
      limit: SIDEBAR_PAGE_SIZE,
      ...(after ? { after } : {})
    });
    const page = response.data ?? [];

    for (const conversation of page) {
      if (seenConversationIds.has(conversation.id)) continue;
      seenConversationIds.add(conversation.id);
      conversations.push(conversation);
    }

    if (!response.has_more || !response.last_id || page.length === 0) {
      return conversations;
    }

    after = response.last_id;
  }
}

export async function listAllConversationProjects(): Promise<ConversationProjectListItem[]> {
  const projects: ConversationProjectListItem[] = [];
  const seenProjectIds = new Set<string>();
  let after: string | undefined;

  while (true) {
    const response = await listConversationProjects({
      limit: SIDEBAR_PAGE_SIZE,
      ...(after ? { after } : {})
    });
    const page = response.data ?? [];

    for (const project of page) {
      if (seenProjectIds.has(project.id)) continue;
      seenProjectIds.add(project.id);
      projects.push(project);
    }

    if (!response.has_more || !response.last_id || page.length === 0) {
      return projects;
    }

    after = response.last_id;
  }
}
