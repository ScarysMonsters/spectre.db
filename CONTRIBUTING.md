# Contributing to spectre.db v2

Thank you for your interest in contributing to **spectre.db**.  
This document explains how to report bugs, suggest features, and submit code changes.

> [!IMPORTANT]
> By contributing to this project, you agree that your contributions are subject to the
> [spectre.db License](./LICENSE). You retain authorship credit for your work, but all
> contributions become part of a project whose intellectual property belongs to **ScarysMonsters**.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Report a Bug](#how-to-report-a-bug)
- [How to Suggest a Feature](#how-to-suggest-a-feature)
- [How to Submit a Pull Request](#how-to-submit-a-pull-request)
- [Development Setup](#development-setup)
- [Project Layout](#project-layout)
- [Code Style Guidelines](#code-style-guidelines)
- [Commit Message Format](#commit-message-format)
- [What We Accept and What We Don't](#what-we-accept-and-what-we-dont)

---

## Code of Conduct

- Be respectful. Harassment, insults, or hostile behavior will result in an immediate ban.
- Stay on topic. Discussions should be relevant to spectre.db.
- Do not spam issues or pull requests with low-effort content.

---

## How to Report a Bug

> [!NOTE]
> Before opening an issue, search [existing issues](https://github.com/ScarysMonsters/spectre.db/issues)
> to make sure it hasn't already been reported.

To report a bug, open a [new issue](https://github.com/ScarysMonsters/spectre.db/issues/new) and include:

1. **Description** — what happened and what you expected to happen.
2. **Reproduction steps** — a minimal code snippet that reproduces the problem.
3. **Environment** — Node.js version (`node -v`), OS/arch, and whether the native
   engine or the JS fallback is in use (`require('@sexfy/spectre.db').hasNativeEngine`).
4. **Error output** — the full error message and stack trace if applicable.

**Example issue title format:**

```
[Bug] db.get() returns null after WAL replay on Windows
```

---

## How to Suggest a Feature

Open a [new issue](https://github.com/ScarysMonsters/spectre.db/issues/new) with:

1. **Use case** — describe the problem you are trying to solve.
2. **Proposed solution** — how you think it should work from the user's perspective.
3. **Alternatives considered** — any workarounds you already tried.

> [!NOTE]
> Feature requests are not guaranteed to be accepted. Priority is given to fixes and
> improvements that align with the project's goal: fast, crash-safe, embeddable storage
> with a stable v1.1.0-compatible API.

**Example issue title format:**

```
[Feature] Add optional TTL parameter to db.set()
```

---

## How to Submit a Pull Request

### 1. Fork and clone

```sh
git clone https://github.com/YOUR_USERNAME/spectre.db.git
cd spectre.db
```

### 2. Create a branch

```sh
git checkout -b fix/wal-replay-empty-lines
git checkout -b feat/set-ttl-support
git checkout -b docs/update-migration-guide
```

### 3. Make your changes

Follow the [Code Style Guidelines](#code-style-guidelines) below.  
Only modify files that are directly relevant to your change.

### 4. Build and test

```sh
npm ci
npm run build        # cargo build + install build/Release/spectre.db-rs.node
npm test             # Jest — native engine
npm run test:fallback  # Jest — JS fallback engine
npm run test:fault     # crash recovery / fault injection
npm run smoke          # end-to-end smoke test
cargo test -p spectre-db-core  # Rust unit tests
```

All suites must pass before a PR is reviewed.

### 5. Commit and push

```sh
git add .
git commit -m "fix: handle empty lines in WAL replay"
git push origin fix/wal-replay-empty-lines
```

### 6. Open a Pull Request

Open a PR against the `main` branch of
[ScarysMonsters/spectre.db](https://github.com/ScarysMonsters/spectre.db).

Include in your PR description:

- **What** the change does.
- **Why** it is needed.
- **How** it was tested.
- A reference to the related issue if one exists (e.g. `Closes #12`).

> [!IMPORTANT]
> Pull requests that do not follow this format or that have no clear justification
> may be closed without review.

---

## Development Setup

Requirements:

- Node.js >= 18.0.0
- Rust stable toolchain (`rustup` — the workspace builds with `cargo`)

```sh
npm ci          # dev dependencies (jest, cross-env)
npm run build   # builds the napi addon into build/Release/
npm test
```

On Windows, `npm run build` uses the bundled `scripts/build.sh` through Git Bash
(available by default on GitHub runners; use Git Bash or WSL locally).

---

## Project Layout

```
spectre.db/
├── index.js               ← JS facade (Database + Table, option mapping, loader)
├── index.d.ts             ← TypeScript types
├── fallback/              ← frozen v1.1.0 JS engine (no v2 format support)
├── crates/
│   ├── spectre-db-core/   ← pure Rust engine (store, formats, WAL, segments, crypto)
│   └── spectre-db-napi/   ← napi-rs bindings
├── npm/                   ← per-platform binary packages (optionalDependencies)
├── scripts/               ← build.sh, smoke.js, check-versions.js
├── tests/                 ← Jest suites (native, fallback, fault injection)
├── docs/FORMATS.md        ← on-disk format specification
├── examples/              ← runnable examples
├── GUIDE.md               ← usage guide
└── README.md
```

---

## Code Style Guidelines

### JavaScript

- **CommonJS only.** No ESM (`import`/`export`).
- **No external runtime dependencies.** The JS layer may only use Node.js built-ins.
- **No comments in source code.** The code should be self-explanatory.
- **No `console.log` in source.** Use events (`this.emit('warn', ...)`).
- **Single quotes**, **2-space indentation**, trailing commas in multi-line literals.

### Rust

- Run `cargo fmt` before committing.
- Keep the public surface of `spectre-db-core` independent from napi types.
- Add unit tests in the crate that owns the logic.

---

## Commit Message Format

| Prefix | When to use |
|---|---|
| `fix:` | A bug fix |
| `feat:` | A new feature |
| `docs:` | Documentation only |
| `refactor:` | Code restructure with no behavior change |
| `perf:` | Performance improvement |
| `chore:` | Tooling, config, or maintenance |

Examples:

```
fix: prevent prototype pollution via constructor segment
feat: add TTL support to LRU cache entries
docs: add paginate() example to README
refactor: extract WALWriter into its own class
perf: replace O(n) cache scan with prefix index
```

---

## What We Accept and What We Don't

### We accept

- Bug fixes with a clear reproduction case.
- Performance improvements that do not add runtime dependencies or break the API.
- Documentation improvements (README, GUIDE, examples, JSDoc).
- New options or features that fit the embedded, file-based scope of the project.
- Compatibility fixes for edge cases in the v1.1.0 API.

### We do not accept

- Pull requests that add runtime npm dependencies to the JS layer.
- ESM rewrites or TypeScript migrations (unless discussed in an issue first).
- Changes that break backward compatibility with the existing API without prior discussion.
- Sharding systems or external coordination mechanisms.
- Pull requests submitted without a description or test verification.

---

## Questions?

Open an issue: [https://github.com/ScarysMonsters/spectre.db/issues](https://github.com/ScarysMonsters/spectre.db/issues)
