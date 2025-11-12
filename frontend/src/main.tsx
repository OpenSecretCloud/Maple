import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./app";
import { waitForPlatform } from "@/utils/platform";
import { LingoProviderWrapper, loadDictionary } from "lingo.dev/react/client";

// Initialize platform detection before rendering
async function initializeApp() {
  // Wait for platform detection to complete
  // This ensures all platform checks are correct from the first render
  await waitForPlatform();

  // Render the app
  const rootElement = document.getElementById("root")!;
  if (!rootElement.innerHTML) {
    const root = createRoot(rootElement);
    root.render(
      <StrictMode>
        <LingoProviderWrapper loadDictionary={(locale) => loadDictionary(locale)}>
          <App />
        </LingoProviderWrapper>
      </StrictMode>
    );
  }
}

// Start the app
initializeApp();
