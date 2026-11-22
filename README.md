# mac-tui-fm

A Neovim-inspired, modal Terminal User Interface (TUI) file manager built in Rust for macOS.

## Features

- **Modal Command Palette**: Fast keyboard-driven navigation (`[f]` Find File, `[d]` Find Directory, `[c]` Find Content, `[r]` Recents, `[h]` History, `[p]` Projects, `[b]` Bucket).
- **Fuzzy, Recents & Content Search**: Powered by `fzf` and `ripgrep` (`rg`), including last-modified Recents (`[r]`, newest first across `search_roots`), with a split-pane syntax-highlighted preview (`Tab` to toggle scroll controls: `1` ↑, `2` ↓, `t` top, `b` bottom).
- **Persistent History (`[h]`)**: Ring buffer of the last 9 accessed/mutated paths persisted across sessions in `~/.config/mac-tui-fm/history.json`.
- **Strict Directory Targeting**: Move (`m`) and Cut (`x`) use `fzf --exact` and automatically create missing target paths via `mkdir -p`.
- **Native macOS Integration**:
  - `NSPasteboard` file copy/paste compatible with Finder (`Cmd+V`) and web uploads.
  - Smart file opening (`open` for binary/media, configurable editor defaulting to `subl` for plaintext).
  - Quick Look (`qlmanage`) and Reveal in Finder (`open -R`).
- **The Bucket**: Persistent staging workspace (`~/.config/mac-tui-fm/bucket.json`) with `1`–`9` jump, `Space` multi-select, batch copy/open, and archive (`a`).
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

- `keybindings.json` — `show_hidden_files`, `editor`, `search_roots`, and custom keybindings.
- `projects.json` — Named project root directories for scoped search (`p`).
- `history.json` — Persisted file history (`h`).
- `bucket.json` — Persisted Bucket staged items (`b`).

## Testing

```bash
cargo test
```
