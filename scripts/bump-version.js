'use strict';

const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const next = process.argv[2];

if (!next || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(next)) {
  console.error('Usage: node scripts/bump-version.js <x.y.z>');
  process.exit(1);
}

function writeJson(file, value) {
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
}

const rootPkgPath = path.join(root, 'package.json');
const rootPkg = JSON.parse(fs.readFileSync(rootPkgPath, 'utf8'));
rootPkg.version = next;
for (const dep of Object.keys(rootPkg.optionalDependencies || {})) {
  rootPkg.optionalDependencies[dep] = next;
}
writeJson(rootPkgPath, rootPkg);

const npmDir = path.join(root, 'npm');
for (const entry of fs.readdirSync(npmDir)) {
  const pkgPath = path.join(npmDir, entry, 'package.json');
  if (!fs.existsSync(pkgPath)) continue;
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  pkg.version = next;
  writeJson(pkgPath, pkg);
}

for (const crate of ['spectre-db-core', 'spectre-db-napi']) {
  const cargoPath = path.join(root, 'crates', crate, 'Cargo.toml');
  const text = fs.readFileSync(cargoPath, 'utf8').replace(/^version = "[^"]+"/m, `version = "${next}"`);
  fs.writeFileSync(cargoPath, text);
}

const lockPath = path.join(root, 'package-lock.json');
if (fs.existsSync(lockPath)) {
  const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
  lock.version = next;
  if (lock.packages && lock.packages['']) {
    lock.packages[''].version = next;
    for (const dep of Object.keys(lock.packages[''].optionalDependencies || {})) {
      lock.packages[''].optionalDependencies[dep] = next;
    }
  }
  for (const key of Object.keys(lock.packages || {})) {
    if (key.startsWith('node_modules/spectre-db-')) {
      delete lock.packages[key];
    }
  }
  writeJson(lockPath, lock);
}

console.log(`Bumped all packages to ${next}`);
console.log('Next: git add -A && git commit -m "release ' + next + '" && git tag v' + next + ' && git push origin main v' + next);
