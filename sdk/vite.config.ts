import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import fs from "fs";
import dts from "vite-plugin-dts";

// Custom plugin for .der file handling
function derPlugin() {
  return {
    name: "vite-der-plugin",
    transform(_src: string, id: string) {
      if (id.endsWith(".der")) {
        // Read the .der file as a buffer
        const buffer = fs.readFileSync(id);

        // Convert the buffer to a Uint8Array
        const uint8Array = new Uint8Array(buffer);

        // Generate code to create and export a Uint8Array
        return {
          code: `export default new Uint8Array([${uint8Array.toString()}]);`,
          map: null
        };
      }
    }
  };
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    derPlugin(),
    dts({
      rollupTypes: true,
      tsconfigPath: "tsconfig.build.json"
    })
  ],
  // Add .der to assetsInclude to ensure it's processed
  assetsInclude: ["**/*.der"],
  resolve: {
    alias: {
      // Ensure absolute imports work correctly
      "@": path.resolve(__dirname, "./src")
    }
  },
  build: {
    lib: {
      entry: path.resolve(__dirname, "src/lib/index.ts"),
      name: "OpenSecretReact",
      fileName: (format) => `opensecret-react.${format}.js`
    },
    rollupOptions: {
      // Externalize React along with its internals
      external: ["react", "react/jsx-runtime", /^react\/.*/],
      output: {
        // Provide global variables to use in the UMD build
        globals: {
          react: "React",
          "react/jsx-runtime": "React"
        }
      }
    }
  }
});
