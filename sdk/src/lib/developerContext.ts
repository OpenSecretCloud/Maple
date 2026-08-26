import { useContext } from "react";
import { OpenSecretDeveloperContext, OpenSecretDeveloperContextType } from "./developer";

export function useOpenSecretDeveloper(): OpenSecretDeveloperContextType {
  const context = useContext(OpenSecretDeveloperContext);
  // React 19 compatibility: Don't check for nullish context since the default value is provided
  return context;
}
