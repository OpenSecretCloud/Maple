import { useEffect, useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { ProjectView } from "@/components/ProjectView";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useLocalState } from "@/state/useLocalState";

export const Route = createFileRoute("/_auth/project/$projectId")({
  component: ProjectPage
});

function ProjectPage() {
  const { projectId } = Route.useParams();
  const { getProjectById } = useLocalState();
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [projectExists, setProjectExists] = useState<boolean | null>(null);

  // Verify project exists
  useEffect(() => {
    async function checkProject() {
      try {
        const project = await getProjectById(projectId);
        setProjectExists(!!project);
      } catch (error) {
        console.error("Error checking project:", error);
        setProjectExists(false);
      }
    }

    checkProject();
  }, [projectId, getProjectById]);

  const toggleSidebar = () => setIsSidebarOpen((prev) => !prev);

  // Show loading state while checking project
  if (projectExists === null) {
    return (
      <div className="flex items-center justify-center h-dvh">
        <p>Loading project...</p>
      </div>
    );
  }

  // Show error if project doesn't exist
  if (projectExists === false) {
    return (
      <div className="flex flex-col items-center justify-center h-dvh gap-4">
        <h1 className="text-xl font-semibold">Project not found</h1>
        <p className="text-muted-foreground">
          The project you're looking for doesn't exist or was deleted.
        </p>
      </div>
    );
  }

  return (
    <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      <Sidebar projectId={projectId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      <main className="flex h-dvh flex-col bg-card/90 backdrop-blur-lg bg-center overflow-hidden">
        {!isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}
        <ProjectView projectId={projectId} />
      </main>
    </div>
  );
}
