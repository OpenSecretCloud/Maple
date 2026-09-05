import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { isTauriDesktop, waitForPlatform } from "@/utils/platform";
import { restoreChatTypographyAtLaunch } from "@/services/chatTypographyPreferences";
import { restoreWorkspaceModeAtLaunch } from "@/services/workspaceModePreference";
import { shouldLoadLegacyDesktopOAuth } from "@/services/desktopOAuthTransport";

// Initialize platform detection before rendering
async function initializeApp() {
  // Restore transcript typography before any asynchronous platform work so a
  // refreshed conversation does not briefly render with the default metrics.
  restoreChatTypographyAtLaunch();

  // Wait for platform detection to complete
  // This ensures all platform checks are correct from the first render
  await waitForPlatform();
  restoreWorkspaceModeAtLaunch(isTauriDesktop());

  // Create the router only after restoring the launch route so its first
  // location snapshot matches the user's saved mode.
  const { default: App } = shouldLoadLegacyDesktopOAuth(window.location)
    ? await import("./legacy/LegacyDesktopOAuthApp")
    : await import("./app");

  // Render the app
  const rootElement = document.getElementById("root")!;
  if (!rootElement.innerHTML) {
    const root = createRoot(rootElement);
    root.render(
      <StrictMode>
        <App />
      </StrictMode>
    );
  }
}

// Start the app
initializeApp();
