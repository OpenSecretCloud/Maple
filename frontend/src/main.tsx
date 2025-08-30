import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./app";
import { initI18n } from "./utils/i18n";

// Initialize localization BEFORE we render anything
initI18n().then(() => {
  console.log('[main] i18n initialized, rendering app...');
  
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
}).catch((error) => {
  console.error('[main] Failed to initialize i18n:', error);
  
  // Still render the app even if i18n fails, but show a warning
  const rootElement = document.getElementById("root")!;
  if (!rootElement.innerHTML) {
    const root = createRoot(rootElement);
    root.render(
      <StrictMode>
        <App />
      </StrictMode>
    );
  }
});
