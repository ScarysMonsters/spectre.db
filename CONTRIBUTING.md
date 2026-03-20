# Contributing to spectre.db

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
3. **Environment** — your Node.js version (`node -v`) and OS.
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
> Feature requests are not guaranteed to be accepted. Priority is given to
> fixes and improvements that align with the project's goal of staying lightweight
> and dependency-free.

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

Use a clear branch name that describes your change:

```sh
git checkout -b fix/wal-replay-empty-lines
git checkout -b feat/set-ttl-support
git checkout -b docs/update-migration-guide
```

### 3. Make your changes

Follow the [Code Style Guidelines](#code-style-guidelines) below.  
Only modify files that are directly relevant to your change.

### 4. Test your changes manually

Run the provided examples to make sure nothing is broken:

```sh
node examples/basic.js
node examples/advanced.js
```

Verify that:

- `db.get`, `db.set`, `db.delete`, `db.has` still work correctly.
- `db.transaction()` (both array and function forms) still rolls back on error.
- The `.snapshot` and `.wal` files are written correctly after operations.
- Reopening the database after `db.close()` produces the same data.

### 5. Commit and push

```sh
git add .
git commit -m "fix: handle empty lines in WAL replay"
git push origin fix/wal-replay-empty-lines
```

### 6. Open a Pull Request

Go to the original repository and open a PR against the `main` branch.

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

No build step is required. spectre.db is pure CommonJS with no dependencies.

```sh
node -v         # must be >= 18.0.0
node index.js   # sanity check
```

All source code lives in:

```
spectre.db/
├── index.js          ← compatibility shim + Database class
├── src/
│   └── engine.js     ← core engine (WAL, LRU, crypto, transactions)
└── examples/
    ├── basic.js
    ├── advanced.js
    └── discord-bot.js
```

---

## Code Style Guidelines

> [!NOTE]
> There is no linter configured. Follow the existing style manually.

- **No external dependencies.** Do not add `require()` calls to packages outside of Node.js core.
- **CommonJS only.** No ESM (`import`/`export`). No TypeScript.
- **No comments in source code.** The code should be self-explanatory. If it isn't, refactor it.
- **No `console.log` in source.** Use `this.emit('warn', message)` or `this.emit('error', err)`.
- **Async by default.** All file I/O must use `fs.promises`. No `fs.readFileSync`, `fs.writeFileSync`, or any other synchronous filesystem call.
- **Single quotes** for strings.
- **2-space indentation.**
- **Trailing commas** in multi-line objects and arrays.
- Keep functions short and focused on a single responsibility.

---

## Commit Message Format

Use the following prefix convention:

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
- Performance improvements that do not add dependencies or break the API.
- Documentation improvements (README, examples, JSDoc).
- New options or features that fit the lightweight, file-based scope of the project.
- Improvements to the compatibility shim for edge cases in the legacy API.

### We do not accept

- Pull requests that add external npm dependencies.
- ESM rewrites or TypeScript migration PRs (unless discussed in an issue first).
- Changes that break backward compatibility with the existing API without prior discussion.
- Sharding systems, clustering, or multi-process coordination mechanisms.
- Pull requests submitted without a description or test verification.
- Code that introduces synchronous filesystem operations.

---

## Questions?

Open an issue: [https://github.com/ScarysMonsters/spectre.db/issues](https://github.com/ScarysMonsters/spectre.db/issues)