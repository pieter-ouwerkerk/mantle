# mantle

CoW-accelerated git worktree hydration for AI coding agents.

mantle creates git worktrees and instantly hydrates them with build artifacts using macOS APFS copy-on-write cloning. A 2GB `node_modules` directory is cloned in milliseconds, using zero additional disk space.

Designed for AI coding agents ([Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://openai.com/index/introducing-codex/)) that work in isolated worktrees, but useful for any workflow that needs fast worktree setup.

## Install

Build from source:

```bash
git clone https://github.com/pieter-ouwerkerk/mantle.git
cd mantle
cargo build --release
# Binary is at target/release/cuttlefish
```

Or install directly:

```bash
cargo install --git https://github.com/pieter-ouwerkerk/mantle cuttlefish-cli
```

## Quick Start

```bash
# 1. Tell mantle what to hydrate (auto-detects from .gitignore)
cuttlefish init

# 2. Create a worktree — automatically hydrated with build artifacts
cuttlefish worktree create --name my-feature --cwd .

# 3. When done, clean up
cuttlefish worktree remove --path ~/.cuttlefish/worktrees/myrepo/my-feature
```

## CLI Reference

| Command | Description |
|---------|-------------|
| `cuttlefish init [path]` | Bootstrap `.worktreeinclude` for a repository |
| `cuttlefish init --show` | Show effective hydration config |
| `cuttlefish init --dry-run` | Preview generated `.worktreeinclude` without writing |
| `cuttlefish worktree create --name <slug>` | Create a worktree with CoW hydration |
| `cuttlefish worktree remove --path <path>` | Remove a worktree |
| `cuttlefish worktree remove --path <path> --force` | Force-remove (even if dirty) |
| `cuttlefish hydrate --worktree <path>` | Hydrate an existing worktree |
| `cuttlefish hook` | Claude Code / Codex hook handler (reads JSON from stdin) |

## `.worktreeinclude`

The `.worktreeinclude` file controls which directories get CoW-cloned into new worktrees. It uses gitignore syntax.

```gitignore
# Hydrate these directories into new worktrees
node_modules
target
.build
venv
dist
```

`cuttlefish init` auto-generates this from your `.gitignore`. You can also create it manually.

### How it works

1. **Scan**: mantle reads `.worktreeinclude` (or falls back to `.gitignore`) to find directories that should be hydrated
2. **Clone**: Each matched directory is cloned using `clonefile(2)` — the macOS APFS copy-on-write syscall
3. **Instant**: CoW cloning is O(1) regardless of directory size. A 2GB directory clones in ~50ms with zero additional disk space
4. **Fallback**: On non-APFS volumes or non-macOS systems, mantle falls back to a regular recursive copy

## Using as a Library

mantle is split into two crates:

- **`mantle-git`** — git operations library (worktrees, CoW cloning, artifact scanning, blame, diff, log, status, etc.)
- **`mantle`** — orchestration layer (hydration: scan → filter → clone)

```toml
# Cargo.toml
[dependencies]
mantle-git = { git = "https://github.com/pieter-ouwerkerk/mantle" }
mantle = { git = "https://github.com/pieter-ouwerkerk/mantle" }
```

```rust
// Hydrate a worktree
let result = mantle::hydrate("/path/to/repo", "/path/to/worktree", &[])?;
println!("Cloned {} directories in {}ms", result.cloned.len(), result.elapsed_ms);

// Or use mantle-git directly for git operations
let branches = mantle_git::git_list_local_branches("/path/to/repo".into())?;
let worktrees = mantle_git::list_worktrees("/path/to/repo".into())?;
```

## Claude Code / Codex Integration

mantle includes a hook handler for AI coding agents. Add to your `.claude/settings.local.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit|Write|NotebookEdit",
        "hooks": [
          { "type": "command", "command": "cuttlefish hook" }
        ]
      }
    ]
  }
}
```

This automatically creates worktrees when an agent tries to edit files in your main working tree, redirecting edits to an isolated worktree.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
