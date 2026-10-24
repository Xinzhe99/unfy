node -e '
const fs = require("node:fs");
const configPath = "apps/desktop/src-tauri/tauri.conf.json";
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
const updater = (config.plugins && config.plugins.updater) || {};
const bundle = config.bundle || {};
const missing = [];
if (!updater.pubkey) {
  missing.push(configPath + ": plugins.updater.pubkey is empty, so a client cannot verify any signature");
}
if (!Array.isArray(updater.endpoints) || updater.endpoints.length === 0) {
  missing.push(configPath + ": plugins.updater.endpoints is empty, so a client cannot find this release");
}
if (!bundle.createUpdaterArtifacts) {
  missing.push(configPath + ": bundle.createUpdaterArtifacts is not set, so no .sig or .app.tar.gz is generated");
}
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  missing.push("repository secret TAURI_SIGNING_PRIVATE_KEY is not set, so nothing can be signed");
}
if (missing.length > 0) {
  console.error("Refusing to build release " + (process.env.GITHUB_REF_NAME || "") + ": the updater is not configured.");
  for (const item of missing) console.error("  missing: " + item);
  console.error("This release would claim auto-update support it cannot deliver, so it is not built.");
  process.exit(1);
}
console.log("Updater configuration is present.");
'
