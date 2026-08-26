import fs from "fs";

// This is just for testing, we have another der loader for vite for actual builds
console.log("DER loader initializing...");

// Register the .der loader with Bun
Bun.plugin({
  name: "der-loader",
  setup(build) {
    build.onLoad({ filter: /\.der$/ }, (args) => {
      console.log("Loading DER file:", args.path);
      const buffer = fs.readFileSync(args.path);
      const uint8ArrayString = Array.from(buffer).join(",");

      return {
        loader: "js",
        contents: `export default new Uint8Array([${uint8ArrayString}]);`
      };
    });
  }
});

// This ensures the plugin is loaded but doesn't export anything
export default {};
