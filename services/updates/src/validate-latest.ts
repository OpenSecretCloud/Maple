import { isLatestRelease } from "./worker";

const path = Bun.argv[2];
if (path === undefined) {
  throw new Error("Usage: bun run validate:latest <latest.json>");
}

let value: unknown;
try {
  value = await Bun.file(path).json();
} catch {
  throw new Error(`${path} is not valid JSON`);
}

if (!isLatestRelease(value)) {
  throw new Error(`${path} is not valid Maple updater metadata`);
}
