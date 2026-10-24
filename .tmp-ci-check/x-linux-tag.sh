export RUNNER_OS_NAME=Linux
export IS_RELEASE=true
export GITHUB_REF_NAME=v9.9.9
node -e '
const fs = require("node:fs");
const path = require("node:path");
const files = [];
(function walk(dir) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.isFile()) files.push(full.split(path.sep).join("/"));
  }
})("target");
const count = (re) => files.filter((f) => re.test(f)).length;
const os = process.env.RUNNER_OS_NAME;
const isRelease = process.env.IS_RELEASE === "true";
const fail = (message) => { console.error(message); process.exit(1); };

let installers;
if (os === "macOS") {
  installers = count(/\/release\/bundle\/dmg\/[^/]+\.dmg$/);
} else if (os === "Windows") {
  installers = count(/\/release\/bundle\/(nsis\/[^/]+\.exe|msi\/[^/]+\.msi)$/);
} else {
  fail("No release artifact rules exist for " + os + "; extend this check before adding the platform.");
}
if (installers === 0) {
  fail("No installer was produced for " + os + ", so this bundle job has nothing to publish.");
}
console.log("Installers: " + installers);

if (!isRelease) {
  console.log("Not a release tag; updater artifacts are optional for this build.");
  process.exit(0);
}

const signatures = count(/\/release\/bundle\/.*\.sig$/);
if (signatures === 0) {
  fail("Release " + (process.env.GITHUB_REF_NAME || "") + " produced no updater signature (.sig). "
    + "Configure plugins.updater.pubkey and endpoints plus bundle.createUpdaterArtifacts, and set TAURI_SIGNING_PRIVATE_KEY.");
}
console.log("Updater signatures: " + signatures);

if (os === "macOS" && count(/\/release\/bundle\/macos\/[^/]+\.tar\.gz$/) === 0) {
  fail("No macOS updater bundle (.app.tar.gz) was produced, so the darwin update key would point at nothing.");
}
'
