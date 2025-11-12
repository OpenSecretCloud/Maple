import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import derPlugin from "./vite-der-plugin";
import lingoCompiler from "lingo.dev/compiler";

// https://vitejs.dev/config/
export default defineConfig(() =>
  lingoCompiler.vite({
    sourceRoot: "src",
    targetLocales: ["es"],
    models: "lingo.dev"
  })({
    plugins: [TanStackRouterVite(), react(), derPlugin()],
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "./src")
      }
    }
  })
);
