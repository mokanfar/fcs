# mac-tui-fm

A Neovim-inspired, modal Terminal User Interface (TUI) file manager built in Rust for macOS.

## Features

- **Modal Command Palette**: Fast keyboard-driven navigation (`[f]` Find File, `[d]` Find Directory, `[c]` Find Content, `[r]` Recents, `[p]` Projects, `[b]` Bucket).
- **1-Second Chord Disambiguation**: Non-blocking Tokio timer resolves overlapping key sequences (e.g., `o` vs `of` / `od`).
- **Fuzzy & Content Search**: Powered by `fzf` and `ripgrep` (`rg`), with a split-pane syntax-highlighted preview (`Tab` to toggle scroll controls: `1` ↑, `2` ↓, `t` top, `b` bottom).
- **Strict Directory Targeting**: Move (`m`) and Cut (`x`) use `fzf --exact` and automatically create missing target paths via `mkdir -p`.
- **Native macOS Integration**:
  - `NSPasteboard` file copy/paste compatible with Finder (`Cmd+V`) and web uploads.
  - Smart file opening (`open` for binary/media, configurable editor defaulting to `subl` for plaintext).
  - Quick Look (`qlmanage`) and Reveal in Finder (`open -R`).
- **The Bucket**: Session-scoped staging workspace with `1`–`9` jump, `Space` multi-select, batch copy/open, and archive (`a`).
- **Workflow Modifiers**: `Additionally` (`a`) chains actions without losing context; `Override` (`Ctrl+O`) swaps Cut ↔ Copy mid-operation.

## Prerequisites

- macOS
- Rust & Cargo (1.84+)
- [`fzf`](https://github.com/junegunn/fzf)
- [`ripgrep`](https://github.com/BurntSushi/ripgrep) (`rg`)
- Optional: [`fd`](https://github.com/sharkdp/fd) (falls back to `find`)

```bash
brew install fzf ripgrep fd
```

## Build & Install

```bash
# Run directly
cargo run

# Or build release binary and install to /usr/local/bin
make install
```

## Configuration

On first launch, default configs are created in `~/.config/mac-tui-fm/`:

- `keybindings.json` — `chord_timeout_ms`, `show_hidden_files`, `editor`, `search_roots`, and custom keybindings.
- `projects.json` — Named project root directories for scoped search (`p`).

## Testing

```bash
cargo test
```
