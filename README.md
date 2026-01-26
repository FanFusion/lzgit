# lzgit

> Because typing `lazygit` is 7 characters. Way too much effort.

## What is this?

A Git TUI for lazy people. Like, *really* lazy.

## Origin Story

Here's the thing:

I spend all day on servers using Claude Code to write code (yes, we're in the vibe coding era now). Every time I want to check a diff, I have to open VSCode Remote SSH, wait forever to connect, just to look at two changed lines.

"Maybe I should build a TUI?"

Then I realized I can't even remember lazygit's shortcuts. Is `s` for stage or stash? What about `S`? You know what, forget it.

And so lzgit was born:

- **lzgit** - 2 fewer characters than lazygit (this matters)
- Almost everything works with **mouse clicks** (because I genuinely cannot remember shortcuts)
- UI stolen from VSCode (because it looks nice)
- Built-in terminal (too lazy to switch windows)
- In-app updates (too lazy to manually install)

## About the Code

I don't know a single line of Rust.

This entire project is 100% written by Claude Code. My contributions:
- Describing what I want
- Saying "that's not right"
- Saying "try again"
- Saying "fine, that works I guess"

## Installation

```bash
# Linux x86_64
curl -fsSL https://github.com/FanFusion/lzgit/releases/latest/download/lzgit-linux-x86_64 -o ~/.local/bin/lzgit && chmod +x ~/.local/bin/lzgit

# Linux ARM64
curl -fsSL https://github.com/FanFusion/lzgit/releases/latest/download/lzgit-linux-aarch64 -o ~/.local/bin/lzgit && chmod +x ~/.local/bin/lzgit

# macOS Intel
curl -fsSL https://github.com/FanFusion/lzgit/releases/latest/download/lzgit-macos-x86_64 -o /usr/local/bin/lzgit && chmod +x /usr/local/bin/lzgit

# macOS Apple Silicon
curl -fsSL https://github.com/FanFusion/lzgit/releases/latest/download/lzgit-macos-aarch64 -o /usr/local/bin/lzgit && chmod +x /usr/local/bin/lzgit

# The AI era way (just ask Claude Code)
claude "install lzgit from https://raw.githubusercontent.com/FanFusion/lzgit/main/README.md"
```

## Usage

```bash
lzgit              # Launch in current directory
lzgit /path/to/repo  # Open specific repo
```

### Shortcuts?

Honestly, I don't remember them all either. But:

- **Just use your mouse**
- `Ctrl+P` - Command palette (stolen from VSCode)
- `T` - Change theme
- `1` `2` `3` - Switch tabs
- `q` - Quit

Everything else... just click it.

## Features

- **Git Tab** - stage/unstage/commit/push/pull, all the usual stuff
- **History Tab** - Browse commits, filter by author
- **Explorer Tab** - File browser with syntax-highlighted preview
- **Terminal Tab** - Built-in terminal, no window switching
- **Conflict Resolution** - Three-way merge view (stolen from VSCode)
- **Themes** - 6 themes, pick your favorite

## Having Issues?

1. Open an issue
2. Or faster: Ask Claude Code to fix it

```
claude "fix this lzgit bug: xxx"
```

After all, it wrote the whole thing anyway.

## Why not just use lazygit?

lazygit is great, but:

1. I can't remember the shortcuts
2. `lazygit` is 7 characters to type
3. I want mouse support
4. I wanted to build something myself (well, have Claude build it)

## License

MIT - Use it however you want. It's not like I wrote the code anyway.

---

*Made with mass, written by Claude Code*
