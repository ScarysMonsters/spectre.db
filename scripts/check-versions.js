'use strict';

const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const rootPkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const version = rootPkg.version;
const errors = [];

for (const [name, range] of Object.entries(rootPkg.optionalDependencies || {})) {
  if (range !== version) {
    errors.push(`package.json optionalDependencies["${name}"] is ${range}, expected ${version}`);
  }
}

const npmDir = path.join(root, 'npm');
for (const entry of fs.readdirSync(npmDir)) {
  const pkgPath = path.join(npmDir, entry, 'package.json');
  if (!fs.existsSync(pkgPath)) continue;
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  if (pkg.version !== version) {
    errors.push(`npm/${entry}/package.json is ${pkg.version}, expected ${version}`);
  }
  if (!pkg.files || !pkg.files.includes('spectre.db-rs.node')) {
    errors.push(`npm/${entry}/package.json is missing the native binary in "files"`);
  }
}

for (const crate of ['spectre-db-core', 'spectre-db-napi']) {
  const cargoPath = path.join(root, 'crates', crate, 'Cargo.toml');
  const cargo = fs.readFileSync(cargoPath, 'utf8');
  const match = cargo.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match || match[1] !== version) {
    errors.push(`crates/${crate}/Cargo.toml is ${match ? match[1] : 'missing'}, expected ${version}`);
  }
}

if (errors.length > 0) {
  console.error('Version mismatch:');
  for (const err of errors) console.error('  - ' + err);
  process.exit(1);
}

console.log(`All package versions match ${version}.`);
