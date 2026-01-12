# TUI Explorer

A modern, fast terminal file explorer written in Rust with mouse support.

![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange)
![License](https://img.shields.io/badge/License-MIT-blue)

## Features

- **Modern UI** - Clean interface with Tokyo Night color scheme and rounded borders
- **Mouse Support** - Click to select files, scroll wheel to navigate
- **File Preview** - Real-time preview of file contents and directory listings
- **Vim-style Navigation** - hjkl keys for power users
- **File Icons** - Nerd Font icons for different file types
- **Fast** - Built with Rust for maximum performance
- **Lightweight** - Single binary, no dependencies at runtime

## Screenshots

```
  Explorer /home/user/projects
╭─ Files (42) ──────────────────────╮╭─ Preview ─────────────────────────╮
│  ▸  src/                         ││  main.rs                          │
│     docs/                        ││  lib.rs                           │
│     tests/                       ││  utils/                           │
│     Cargo.toml            1.2K   ││                                   │
│     README.md             3.4K   ││                                   │
│     .gitignore            128B   ││                                   │
╰───────────────────────────────────╯╰───────────────────────────────────╯
 ↑↓/jk nav  Enter/l open  Backspace/h back  g/G top/end  q quit
```

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/tui-explorer-rs.git
cd tui-explorer-rs

# Build and install
cargo build --release
sudo cp target/release/tui-explorer-rs /usr/local/bin/te
```

### Requirements

- Rust 1.70 or later
- A terminal with true color support (recommended)
- Nerd Font for file icons (optional but recommended)

## Usage

```bash
# Open current directory
te

# Open specific directory
te /path/to/directory

# Open home directory
te ~
```

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `PageUp` | Move up 10 items |
| `PageDown` | Move down 10 items |
| `g` | Go to first item |
| `G` | Go to last item |

### Actions

| Key | Action |
|-----|--------|
| `Enter` / `l` | Enter directory |
| `Backspace` / `h` | Go to parent directory |
| `q` | Quit |

### Mouse

| Action | Effect |
|--------|--------|
| Left Click | Select file/directory |
| Scroll Up | Move selection up |
| Scroll Down | Move selection down |

## Color Scheme

TUI Explorer uses the **Tokyo Night** color scheme:

| Element | Color |
|---------|-------|
| Directories | Blue (#7aa2f7) |
| Files | Light gray (#c0caf5) |
| Symlinks | Purple (#bb9af7) |
| Executables | Green (#9ece6a) |
| Hidden files | Dim gray (#565f89) |
| Selected item | Blue background |

## File Icons

Icons are displayed for common file types (requires Nerd Font):

| Icon | File Types |
|------|------------|
|  | Directories |
|  | Symlinks |
|  | Rust (.rs) |
|  | TypeScript (.ts, .tsx) |
|  | JavaScript (.js, .jsx) |
|  | Python (.py) |
|  | Go (.go) |
|  | JSON (.json) |
|  | Config (.toml, .yaml, .yml) |
|  | Markdown (.md) |
|  | Shell (.sh, .bash) |
|  | Lock files |
|  | Other files |

## Configuration

Currently, TUI Explorer uses built-in defaults. Configuration file support is planned for future releases.

## Project Structure

```
tui-explorer-rs/
├── src/
│   └── main.rs      # Main application code
├── Cargo.toml       # Project dependencies
├── README.md        # This file
└── LICENSE          # MIT License
```

## Dependencies

- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [crossterm](https://github.com/crossterm-rs/crossterm) - Cross-platform terminal manipulation

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## Roadmap

- [ ] Configuration file support
- [ ] Custom themes
- [ ] File operations (copy, move, delete)
- [ ] Bookmarks
- [ ] Search/filter functionality
- [ ] Multiple panels
- [ ] Integration with system file opener

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Inspired by [lazygit](https://github.com/jesseduffield/lazygit) UI design
- Built with [ratatui](https://ratatui.rs/)
- Color scheme based on [Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme)
