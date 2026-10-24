export MATRIX_OS=windows-latest; export IS_RELEASE=true
node -e "
const fs = require('node:fs');
const path = require('node:path');
const files = [];
(function walk(dir) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.isFile()) files.push(full.split(path.sep).join('/'));
  }
})('target');
const count = (re) => files.filter((f) => re.test(f)).length;
const os = process.env.MATRIX_OS;
const isRelease = process.env.IS_RELEASE === 'true';
const fail = (message) => { console.error(message); process.exit(1); };

const installers = os === 'macos-14'
  ? count(/\/release\/bundle\/dmg\/[^/]+\.dmg$/)
  : count(/\/release\/bundle\/(nsis\/[^/]+\.exe|msi\/[^/]+\.msi)$/);
if (installers === 0) {
  fail('No installer was produced for ' + os + ', so this bundle job has nothing to publish.');
}
console.log('Installers: ' + installers);

if (!isRelease) {
  console.log('Not a release tag; updater artifacts are optional for this build.');
  process.exit(0);
}

const signatures = count(/\/release\/bundle\/.*\.sig$/);
if (signatures === 0) {
  fail('Release ' + (process.env.GITHUB_REF_NAME || '') + ' produced no updater signature (.sig). '
    + 'Configure plugins.updater.pubkey and endpoints plus bundle.createUpdaterArtifacts, and set TAURI_SIGNING_PRIVATE_KEY.');
}
console.log('Updater signatures: ' + signatures);

if (os === 'macos-14' && count(/\/release\/bundle\/macos\/[^/]+\.tar\.gz$/) === 0) {
  fail('No macOS updater bundle (.app.tar.gz) was produced, so the darwin update key would point at nothing.');
}
"
