---
name: project-tooling
description: >
  Use this skill when the user asks to verify code changes, run lint/typecheck,
  or asks "what command should I run" / "is this red". Triggers on mentions of
  "verify", "typecheck", "lint", "compile", "build", or right after the agent
  edits a source file. Detects the project's toolchain (Rust / TypeScript /
  Python / Go) from marker files and gives the agent the right CLI command
  for structured diagnostics — beats running blind `cargo build` or scrolling
  through giant text logs.
---

# Project Tooling

Goal: when the agent finishes editing source code, run the **correct** project-level
verification command (one per detected language) and read the **structured** output
so the agent can react to errors precisely.

## Step 1 — Detect project type

Use the `read_file` tool to check for marker files at the workspace root:

| Marker | Language |
|--------|----------|
| `Cargo.toml` | Rust (look for `[workspace]` to know if it's a multi-crate workspace) |
| `package.json` + `tsconfig.json` | TypeScript |
| `package.json` (no tsconfig) | JavaScript |
| `pyproject.toml` / `setup.py` / `requirements.txt` | Python |
| `go.mod` | Go |

Multiple markers can co-exist (e.g. Rust workspace with a TypeScript subdir).
Run the verification step for **every** detected language whose files were touched.

Also scan for these to know which lint binaries the project has configured:

- `.eslintrc*` / `eslint.config.*` → ESLint
- `biome.json` / `biome.jsonc` → Biome
- `pyrightconfig.json` or `pyright` mentioned in `pyproject.toml` → Pyright
- `ruff.toml` or `[tool.ruff]` in `pyproject.toml` → Ruff
- `pnpm-lock.yaml` / `yarn.lock` / `bun.lockb` → use that package manager (else npm)

## Step 2 — Run the right verification command

Use the `execute_shell` tool with **structured-output flags** when the tool supports them.
Structured JSON is much cheaper than free-form text on the agent's context.

### Rust

| Project shape | Command |
|---------------|---------|
| Workspace | `cargo check --workspace --lib --message-format=json` |
| Single crate | `cargo check --message-format=json` |
| Stricter check (run **after** check passes) | replace `check` with `clippy` |

Each line of output is a JSON object. The lines you care about have
`"reason":"compiler-message"` and contain a `message` field with `level`
(`error` / `warning`), `code.code`, `message`, and `spans[]` (each with
`file_name`, `line_start`, `column_start`, `is_primary`). Filter to primary spans;
ignore `note` and `help` levels except as supplementary suggestions inside
`message.children[]`.

### TypeScript / JavaScript

```sh
# typecheck — choose the first that exists
pnpm typecheck            # if scripts.typecheck in package.json
pnpm exec tsc --noEmit    # else, with tsconfig.json
```

`tsc` output is plain text, line per error:
```
src/foo.ts(5,8): error TS2304: Cannot find name 'x'.
```

```sh
# lint — pick by what's configured
pnpm exec biome ci --reporter=json .   # if biome.json / @biomejs/biome installed
pnpm exec eslint --format=json .       # if .eslintrc / eslint installed
```

Both produce JSON with file paths + line/column + rule id + message + optional
`fix`/`suggestion`. Use the rule id to know what convention violation triggered.

### Python

```sh
ruff check --output-format=json .          # if ruff configured
pyright --outputjson                        # if pyright configured
mypy --show-column-numbers --show-error-codes .   # else, if mypy configured
```

Ruff JSON: `[{filename, code, message, location:{row,column}, fix:{message}}]`.
Pyright JSON: `generalDiagnostics[]` with `file`, `severity`, `message`, `range.start`.

### Go

```sh
go vet ./...                  # always available with go toolchain
staticcheck ./...             # if installed (optional)
```

`go vet` writes diagnostics to **stderr** in the form:
`./pkg/foo.go:12:3: missing return at end of function`.

## Step 3 — Read & summarize for the user

Focus on:

1. **Errors first**, warnings if no errors. Skip `note` / `info` / `hint` unless
   the user asks.
2. **File:line:column** as the precise anchor. Don't paraphrase the location.
3. **Suggestion fields** (clippy `children[].level=="help"`, ESLint `fix.text`,
   Ruff `fix.message`) — read these and include the proposed fix when present.
4. **Compress before quoting**. Don't paste the full JSON output back to the user.
   Output one line per diagnostic, format like:
   `src/foo.rs:42:8 [E0308 error] mismatched types: expected String, found &str`.

## Notes

- **No structured-output flag exists** for `tsc` and `go vet` — parse the text yourself.
- If a tool isn't installed (`command not found`), don't try to install it without
  asking. Offer to add the config and tooling (`pnpm add -D @biomejs/biome`,
  `pip install ruff`) but require user confirmation.
- This skill is project-agnostic. If the project has an `AGENTS.md` with more
  specific commands (e.g. a custom `make verify` target), prefer that.
