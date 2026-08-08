export type AgentProjectFolderRevealer = (projectPath: string) => Promise<void>;

async function revealProjectFolderWithTauri(projectPath: string): Promise<void> {
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(projectPath);
}

export async function revealAgentProjectFolder(
  projectPath: string,
  reveal: AgentProjectFolderRevealer = revealProjectFolderWithTauri
): Promise<void> {
  if (typeof projectPath !== "string" || projectPath.trim().length === 0) {
    throw new Error("Project folder path is required.");
  }

  await reveal(projectPath);
}
