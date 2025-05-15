import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import derPlugin from "./vite-der-plugin";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [TanStackRouterVite(), react(), derPlugin()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src")
    }
  },
  // TODO: REMOVE AFTER TESTING - This configuration is only for ngrok testing
  server: {
    // Allow specific hosts for development
    allowedHosts: [
      'localhost',
      'dea1-37-19-200-146.ngrok-free.app' // Remove this line after testing
    ]
  }
});
