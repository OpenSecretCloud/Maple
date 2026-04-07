import { createFileRoute } from "@tanstack/react-router";
import { ProjectDetailPage } from "@/components/projects/ProjectDetailPage";

export const Route = createFileRoute("/_auth/project/$projectId")({
  component: ProjectRoute
});

function ProjectRoute() {
  const { projectId } = Route.useParams();
  return <ProjectDetailPage projectId={projectId} />;
}
