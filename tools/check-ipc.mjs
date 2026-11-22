import { readdirSync, readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");
const collect = (source, pattern) => new Set(Array.from(source.matchAll(pattern), (match) => match[1]));
const contract = collect(read("src/ipc.d.ts"), /\b__TAURI_INVOKE(?:<[^\n]*>)?\(["']([\w:-]+)["']/g);
if (!contract.size) throw new Error("src/ipc.d.ts contains no generated commands");

const errors = [];
const sourceRoot = new URL("src/", root);
for (const path of readdirSync(sourceRoot, { recursive: true }).filter((name) => /\.(js|html)$/.test(name))) {
  const source = read(`src/${path}`);
  const names = collect(source, /\binvoke\(\s*["'`]([\w:-]+)["'`]/g);
  // captureAsk takes either the default screenshot command or a literal override.
  for (const name of collect(source, /\bcaptureAsk\([^\n"'`]*["'](capture_\w+)["']/g)) names.add(name);
  for (const name of names) {
    if (!contract.has(name)) errors.push(`${path}: invoke command ${name} is missing from src/ipc.d.ts`);
  }
}

for (const name of collect(read("tools/mock-tauri.js"), /^\s*case\s+["']([\w:-]+)["']\s*:/gm)) {
  if (!contract.has(name)) errors.push(`tools/mock-tauri.js: mock command ${name} is missing from src/ipc.d.ts`);
}
if (errors.length) {
  console.error(errors.join("\n"));
  process.exitCode = 1;
} else {
  console.log(`Checked frontend and mock commands against ${contract.size} generated commands.`);
}
