# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build                    # Dev build
cargo build --release          # Release (LTO enabled)
cargo check                    # Fast type checking
cargo run                      # Run dev build
cargo run -- /path/to/dir      # Run with directory argument

cargo fmt                      # Format code
cargo fmt --check              # Check formatting
cargo clippy -- -D warnings    # Lint with strict warnings

cargo test                     # Run all tests
cargo test test_name           # Run single test
```

### Local Install
After changes, update the local binary so `te` is runnable:
```bash
cargo build --release && install -m 755 target/release/te ~/.local/bin/te
```

## Architecture

This is a terminal file explorer with Git integration, built with ratatui/crossterm.

### Module Structure

- **main.rs**: Application core - `App` state, event loop, UI rendering, `FileEntry` type, theme system with 5 color palettes (Mocha, Tokyo Night, Gruvbox, Nord, Dracula)
- **git.rs**: Git state management (`GitState`), diff rendering (`GitDiffRow`, `build_side_by_side_rows`), status parsing
- **git_ops.rs**: Git command wrappers - history listing, diff generation, stage/unstage operations, branch/stash management
- **branch.rs**: Branch picker UI (`BranchUi`), fuzzy search scoring
- **commit.rs**: Commit message editor state (`CommitState`), cursor management
- **conflict.rs**: Merge conflict parsing and resolution (`ConflictFile`, `ConflictResolution`)
- **highlight.rs**: Syntax highlighting via syntect for file preview
- **openrouter.rs**: AI commit message generation via OpenRouter API

### Core Patterns

**Event Loop**: `terminal.draw()` renders state -> `event::read()` blocks for input -> handlers modify `App` -> loop until `app.should_quit`

**Git Commands**: All git operations use `run_git()` in git_ops.rs which disables interactive prompts via env vars (`GIT_TERMINAL_PROMPT=0`, etc.)

**Error Handling**: Use `?` operator in main, `.ok()` for optional failures, `let-else` for early returns

### Key Types

- `App`: Main application state container
- `FileEntry`: File/directory with metadata
- `GitState`: Git tab state (branch, entries, diff)
- `Palette`: Theme colors (bg, fg, accent, diff colors)

## Adding Features

**Add keybinding**: Add match arm in `Event::Key` handler in main.rs, implement method on `App`, update help bar

**Add file icon**: Add match arm in `get_icon()` function

**Add theme color**: Add to `Palette` struct and `palette()` function in the `theme` module

## Environment Variables

- `OPENROUTER_API_KEY`: Required for AI commit message generation
- `OPENROUTER_MODEL`: Optional, defaults to `openai/gpt-5.2`
