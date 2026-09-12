# Releasing

This document describes how versions are bumped, how prebuilt binaries are
produced and how packages are published to npm.

## Packages

| Package | Contents |
|---|---|
| [`@sexfy/spectre.db`](https://www.npmjs.com/package/@sexfy/spectre.db) | JS facade, types, fallback engine, docs, examples |
| `spectre-db-linux-x64-gnu` | Linux x64 glibc prebuild |
| `spectre-db-linux-x64-musl` | Linux x64 musl prebuild |
| `spectre-db-darwin-x64` | macOS x64 prebuild |
| `spectre-db-darwin-arm64` | macOS arm64 prebuild |
| `spectre-db-win32-x64-msvc` | Windows x64 prebuild |

The root package declares the platform packages as `optionalDependencies`, and
the runtime loader resolves them automatically. All six packages always share
the same version.

## Version bump

```sh
node scripts/bump-version.js 2.0.1
```

The script updates:

- root `package.json` (version + `optionalDependencies`),
- the five `npm/spectre-db-*/package.json`,
- `crates/spectre-db-core/Cargo.toml` and `crates/spectre-db-napi/Cargo.toml`,
- `package-lock.json`.

Consistency is enforced by:

```sh
npm run check-versions   # also runs automatically before publishing (prepublishOnly)
```

## Release flow

```sh
node scripts/bump-version.js 2.0.1
git add -A
git commit -m "release 2.0.1"
git tag v2.0.1
git push origin main v2.0.1
```

Pushing the `v*` tag triggers the **Release** workflow:

1. **test** — installs dependencies, runs Rust unit tests, builds the native
   module and runs the full Jest suite (native, fallback, fault injection).
2. **build** — builds the napi addon for the five targets:

   | Target | Runner | Artifact |
   |---|---|---|
   | `x86_64-unknown-linux-gnu` | ubuntu-latest | `libspectre_db_napi.so` |
   | `x86_64-unknown-linux-musl` | ubuntu-latest (cargo-zigbuild + Zig) | `libspectre_db_napi.so` |
   | `x86_64-apple-darwin` | macos-latest | `libspectre_db_napi.dylib` |
   | `aarch64-apple-darwin` | macos-14 | `libspectre_db_napi.dylib` |
   | `x86_64-pc-windows-msvc` | windows-latest | `spectre_db_napi.dll` |

   Each artifact is copied into `npm/<package>/spectre.db-rs.node` and uploaded.
3. **publish** — downloads the artifacts, checks versions and publishes:
   - the platform packages first,
   - then the root package.
   Already-published versions are skipped, so re-running a release is safe.

The workflow can also be started manually (`workflow_dispatch`) from the
Actions tab.

## Publishing authentication

The publish job uses **npm trusted publishing (OIDC)** — no `NPM_TOKEN`
secret. Requirements:

- `permissions: id-token: write` in the workflow (already set),
- a GitHub environment named `npm` on the publish job (already set),
- a **Trusted Publisher** configured on npm for **each of the six packages**:

  | Field | Value |
  |---|---|
  | Provider | GitHub Actions |
  | Organization or user | `ScarysMonsters` |
  | Repository | `spectre.db` |
  | Workflow filename | `release.yml` |
  | Environment name | `npm` |
  | Allowed actions | allow direct `npm publish` |

- npm CLI >= 11.5.1 and Node >= 22.14 — the publish job uses Node 24 and runs
  `npm install -g npm@latest`.

The package's `repository` field must match the GitHub repository
(`https://github.com/ScarysMonsters/spectre.db`), which it does.

> The first publish of a new package cannot use OIDC (the package must exist to
> configure its trusted publisher). Publish the first version manually with
> `npm publish --access public` (2FA), then configure the trusted publisher.

## Local build

Prerequisites: Rust stable toolchain and a bash shell (Git Bash or WSL on
Windows).

```sh
npm install
npm run build      # scripts/build.sh → cargo build -p spectre-db-napi --release
                   # then installs build/Release/spectre.db-rs.node
npm test
```

Runtime resolution order: `build/Release/` → `prebuilds/` → platform npm
packages → JavaScript fallback.

## Manual publish (fallback)

If CI publishing is unavailable:

```sh
# platform packages first
npm publish ./npm/spectre-db-linux-x64-gnu --access public
npm publish ./npm/spectre-db-linux-x64-musl --access public
npm publish ./npm/spectre-db-darwin-x64 --access public
npm publish ./npm/spectre-db-darwin-arm64 --access public
npm publish ./npm/spectre-db-win32-x64-msvc --access public

# then the root package
npm publish --access public
```

Make sure each `npm/<package>/spectre.db-rs.node` binary is present (download
the CI artifacts or build locally).

## Repository

- GitHub: <https://github.com/ScarysMonsters/spectre.db>
- `main` hosts the v2 line.
- `v1-legacy` preserves the original v1.1.0 JavaScript implementation
  (also available as npm `spectre.db@1.1.0`).

## Release checklist

- [ ] `node scripts/bump-version.js X.Y.Z`
- [ ] Update `CHANGELOG.md`
- [ ] `npm test`, `npm run test:fallback`, `npm run test:fault`, `cargo test -p spectre-db-core`
- [ ] `npm run check-versions`
- [ ] Commit, tag `vX.Y.Z`, push `main` and the tag
- [ ] Watch the **Release** workflow (test → build → publish)
- [ ] Verify: `npm view @sexfy/spectre.db version`
