# **Technical Specification: macOS Modal TUI File Manager**

System Architecture, Keybinding Logic, and Implementation Roadmap for a Neovim-Inspired Command-Driven File Manager.

# **1\. Overview & Architecture**

This specification details the design for a high-performance Terminal User Interface (TUI) file manager tailored exclusively for macOS. The system leverages modal workflow patterns inspired by Neovim (`nvim`), prioritizing rapid keyboard-driven navigation, structured action menus, and seamless integration with the native macOS desktop environment.

## **Core Technology Stack**

* **Implementation Language**: Rust (Edition 2021\) for memory safety, low resource overhead, and precise async event loop processing.  
* **TUI Engine**: `ratatui` for rich terminal rendering, UI state layouts, and widget trees.  
* **Terminal I/O Backend**: `crossterm` for cross-terminal event handling, raw mode control, and ANSI escape sequence generation.  
* **External Search Engines**:  
  * `fzf` (Fuzzy Finder): Invoked asynchronously for fast directory-level and file-level path matching.  
  * `ripgrep` (`rg`): Used for content-based searches across configured project paths.  
* **Build & Distribution System**:  
  * Managed via standard Cargo toolchain (`cargo build --release`).  
  * Supplemental `Makefile` for system-level installation targets (`/usr/local/bin`), shell completions, and manpage generation.

| Layer | Component | Specification / Dependency |
| :---- | :---- | :---- |
| **Presentation** | Terminal Renderer | `ratatui` \+ `crossterm` |
| **Logic Engine** | State & Input Parser | Custom Async Event Loop (Tokio/futures) |
| **Search Subsystem** | File / Text Indexing | `fzf` integration, `ripgrep` (`rg`) |
| **OS Interface** | macOS Integration | Native pasteboard (`pbcopy`/AppKit APIs), Quick Look (`qlmanage`) |
| **Configuration** | Settings & Keybindings | `serde_json` config schemas in `~/.config/mac-tui-fm/` |

# **2\. Configuration & Keybinding System**

The application separates keybinding layout definitions from core runtime logic using a JSON-based schema.

## **Keybinding Configuration Schema**

The keybinding engine supports single keys and modifier combinations (`cmd + f`, `ctrl + p`) mapped directly to domain commands.

```
{
  "settings": {
    "show_hidden_files": false,
    "editor": "subl",
    "search_roots": ["~/Desktop", "~/Downloads", "~/Documents"]
  },
  "keybindings": {
    "home_screen": {
      "f": "search.find_file",
      "d": "search.find_directory",
      "c": "search.find_content",
      "r": "search.recents",
      "h": "search.history",
      "p": "search.projects",
      "b": "bucket.open"
    },
    "action_menu": {
      "m": "action.move",
      "c": "submenu.copy",
      "x": "action.cut",
      "e": "action.edit",
      "b": "action.bucket",
      "o": "submenu.open",
      "p": "action.paste",
      "h": "nav.home"
    }
  }
}
```

## **Projects Configuration**

Users define accessible project root paths in `~/.config/mac-tui-fm/projects.json`. Triggering Project Search mode (`p`) launches a fuzzy search (`fzf`) over the configured project list (matching by project `name` or `path`). Selecting a project sets the active search scope to that project's directory and presents the scoped command menu (`f`, `d`, `c`, `r`, `h`, `p`, `b`, with `Esc` to clear project scope) restricted to that project root:

```
{
  "projects": [
    { "name": "System Engine", "path": "~/Developer/Rust/tui-file-manager" },
    { "name": "Web Services", "path": "~/Work/Backend/microservices" }
  ]
}
```

# **3\. Initial Menu (Home Screen)**

Upon launch, the application opens into a command palette style interface showing active state and mnemonic modal entry points:

```
┌────────────────────────────────────────────────────────────────────────┐
│ File Scout Context v0.1.0                                             │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   [f]   Find File          Fuzzy search files via fzf / ripgrep         │
│   [d]   Find Directory     Fuzzy search system directories             │
│   [c]   Find Content       Fuzzy search file contents via rg + fzf     │
│   [r]   Recents            Search recently modified files (newest desc) │
│   [h]   History            Browse persistent file history (1-9)        │
│   [p]   Projects           Search strictly within configured roots     │
│   [b]   Bucket             Open the persistent Bucket staging workspace│
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

* `f` (**Find File**): Opens fuzzy finder over all files in the active scope using `fzf`/`ripgrep`.  
* `d` (**Find Directory**): Filters strictly for directory paths.  
* `c` (**Find Content**): Prompts for directory scope selection from `settings.search_roots` (`1`–`9` to select an individual directory, or `0` to search within **all** specified directories). Once a scope is chosen, it waits for at least 3 characters of input before executing `ripgrep` (`rg`) with `fzf` to fuzzy-search file contents within that scope. Search results are displayed in a split layout with a **preview pane visible to the right of the results pane**.
  * **Results Navigation**: Strictly `Up`/`Down` arrow keys + `Enter` to select a single file (no `1`–`9` result pickers and no `Space` multi-selection across any fuzzy search mode: `f`, `d`, `c`, `r`, `p`).
  * **Path Truncation**: In the file/results pane, any file path longer than **40 characters** must be middle-truncated with `...`—preserving the leading root segments and trailing path segments (e.g., `/Users/dan/.../code/asdf/`).
  * **Preview Pane Controls**: `1` (press/hold) smooth-scrolls up, `2` (press/hold) smooth-scrolls down, `t` jumps to top, `b` jumps to bottom.  
* `r` (**Recents**): Lists files sorted by last-modified timestamp (newest descending) across the directories configured in `settings.search_roots` (or the active project root when inside a **Project Scope**). Results are live-searchable via `fzf` (preserving newest-first ordering via `--no-sort`) with a split-pane preview.  
* `h` (**History**): Displays up to 9 recently accessed or mutated paths (`1`–`9`), persisted across sessions in `~/.config/mac-tui-fm/history.json`.  
* `p` (**Projects**): Fuzzy-searches the configured project list in `projects.json`. Selecting a project sets the active directory scope to that project root and re-displays the command menu (`f`, `d`, `c`, `r`, `h`, `p`, `b`, `Esc` to clear scope) scoped to that project.  
* `b` (**Bucket**): Opens the persistent Bucket staging workspace (`~/.config/mac-tui-fm/bucket.json`).

# **4\. Post-Selection File Context & Action Submenus**

Selecting a file or directory entry from search (`f`, `d`, `c`, `r`) or `h` (**History**) updates the **Active Context** and transitions the UI directly to the **Action Menu**. (By contrast, selecting a project in `p` (**Projects**) sets the active search scope and presents the scoped command menu: `f`, `d`, `c`, `r`, `h`, `p`, `b`, with `Esc` to return to global scope.)

```
Active File Context: /Users/dev/Projects/engine/src/main.rs
┌────────────────────────────────────────────────────────────────────────┐
│ ACTION MENU                                                            │
├────────────────────────────────────────────────────────────────────────┤
│  [m] Move        [c] Copy Menu      [x] Cut          [e] Edit          │
│  [b] Bucket      [o] Open Menu      [p] Paste*       [h] Home          │
└────────────────────────────────────────────────────────────────────────┘
```
*\*Note: `[p] Paste` is only shown when the active context is a **directory** (omitted when the active context is a file).*

## **Action Mapping Table**

| Key | Target Action / Submenu | Description |
| :---- | :---- | :---- |
| `m` | **Move** | Opens a **strict** directory finder (`fzf --exact` / strict path matching, not loose fuzzy matching) scoped to configured directories (`settings.search_roots`) so new paths are not hijacked by opportunistic matches. If no existing directory strictly matches the typed path (e.g., `desktop/non-existent-dir`), pressing `Enter` creates it via `mkdir -p` and moves the file into it. |
| `c` | **Copy Submenu** | Opens specialized copy modes (Name, Content, Native File, Path). |
| `x` | **Cut** | Marks context for cut-and-paste relocation and immediately opens the **strict** directory finder (`fzf --exact` / strict path matching) scoped to `settings.search_roots`. If no existing directory strictly matches the typed path (e.g., `desktop/non-existent-dir`), pressing `Enter` creates it via `mkdir -p` and moves/pastes the file into it. |
| `e` | **Edit** | Opens the active file in the configured editor (default: Sublime Text / `subl`, configurable via `settings.editor` in JSON). |
| `b` | **Bucket** | Adds/stages the active file context into the persistent Bucket (deduplicating if already present) and returns to the Main Menu (Home Screen). |
| `o` | **Open Submenu** | Opens submenu: `(f)` **File**, `(d)` **Directory** (opens containing folder), or `(q)` **Quick Look**. |
| `p` | **Paste** | Pastes current clipboard contents or pending cut/copy file context (shown only when active context is a directory). |
| `h` | **Home** | Clears active context and returns to the Home Screen. |

## **Nested Submenu Specifications**

### **Copy Submenu (`c`)**

* **Copy File Name**: Writes only the filename string (e.g., `main.rs`) to the macOS system clipboard.  
* **Copy Contents**: Reads file raw bytes and copies plain text content to clipboard.  
* **Copy File**: Copies the native binary file payload/reference to the macOS pasteboard (`NSPasteboard` / `pasteboard` crate), enabling direct pasting into native macOS Finder windows.  
* **Copy Directory (`dir`)**: Opens **Path Submenu**:  
  * **Relative Path**: Copies file path relative to the active project root directory (e.g., `src/main.rs`).  
  * **Absolute Path**: Copies full canonical path (e.g., `/Users/dev/Projects/engine/src/main.rs`).

### **Open Submenu (`o`)**

* **File (`f`)**: If the file is plaintext (e.g., `.rs`, `.js`, `.txt`, `.md`), opens it in the configured editor (`settings.editor`, defaulting to `subl` / Sublime Text). Otherwise, launches it in the default macOS application associated with its UTI type via `open`.  
* **Directory (`d`)**: Opens the parent folder containing the target file.

# **5\. Persistent State & Workflow Modifiers**

## **The Bucket**

The **Bucket** is a persistent virtual workspace and staging directory managed by the application, preserved across sessions in `~/.config/mac-tui-fm/bucket.json`.

* Acts as a scratchpad for files collected across disparate directories, displayed in a numbered `1`–`9` list where pressing `1`–`9` selects a staged entry.  
* Automatically restores staged items and staging symlinks on startup, and persists additions (`b`) and archive clears (`a`) to `~/.config/mac-tui-fm/bucket.json`.  
* Supports multi-selection via `Space` (toggle selection on the highlighted item) and `Up`/`Down` arrow key navigation, allowing batch actions across multiple selected files.  
* Supports staging files and acting on them directly from the Bucket interface.  
* Exposes native macOS pasteboard exports for pasting files or images directly into Finder or web apps (e.g., `Cmd+V` upload to Imgur).

### **Bucket Action Keybindings**

| Key | Action / Submenu | Description |
| :---- | :---- | :---- |
| `c` | **Copy Submenu** | Opens copy submenu:<br>• `(f)` **Copy File**: Copies native file/binary payload to `NSPasteboard` (e.g., `Cmd+V` into Finder or Imgur).<br>• `(c)` **Copy Contents**: Copies plaintext file contents (`.js`, `.txt`, `.md`, non-binary).<br>• `(d)` **Copy Dir Path**: Opens submenu with `(r)` Relative path and `(f)` Full (absolute) path. |
| `o` | **Open Submenu** | Opens submenu:<br>• `(f)` **Finder**: Reveals/opens in macOS Finder.<br>• `(o)` **Open File**: Opens file with default macOS application.<br>• `(d)` **Open Directory**: Opens the Bucket's staging directory. |
| `a` | **Archive** | Clears the contents of the Bucket and updates persisted state. |
| `h` | **Home** | Returns to the Home Screen. |

## **Workflow Modifiers**

### **Additionally (`a`)**

The `Additionally` key acts as a chaining modifier. When appended to an action execution:

1. The executed command processes immediately.  
2. The active file context is preserved in memory rather than cleared.  
3. The UI automatically re-opens the **Action Submenu** for subsequent operations on the same context without requiring re-selection.

### **Override (`Ctrl+O`)**

The `Override` key provides mid-operation intent modification. If an operation is currently pending (e.g., a file is marked for a **Cut** operation):

* Pressing `Ctrl+O` allows the user to switch the pending state to **Copy** (or vice versa) without aborting the selection flow or clearing the context.

### **Persistent History (`h`)**

* Tracks files, paths, and directories accessed or mutated by the user across sessions, persisted to `~/.config/mac-tui-fm/history.json`.  
* Each accessed or mutated path (including both the original source path and the resulting destination path after a move) is recorded as its own distinct entry.  
* Maintained in a persistent ring buffer displaying up to **9 items** (`1`–`9`) via `h`; pressing a number key `1`–`9` (or `Up`/`Down` + `Enter`) immediately selects the corresponding entry.  
* If a selected history entry no longer exists on disk, the UI displays a **"File does not exist"** prompt and waits for follow-up input (`h` for History, `r` for Recents, or `Esc` for Home).

### **Recents (`r`)**

* Scans the configured directories in `settings.search_roots` (or the active project root when inside a **Project Scope**) and displays files sorted by last-modified timestamp (**newest descending**).  
* Supports live fuzzy filtering via `fzf --no-sort` (preserving newest-first ordering) alongside the split-pane file preview.

# **6\. Proposed Rust Project Structure**

The project is structured into clear modular components:

```
mac-tui-file-manager/
├── Cargo.toml
├── Makefile
├── config/
│   ├── keybindings.json
│   └── projects.json
└── src/
    ├── main.rs           # Entry point, terminal initialization, event loop
    ├── app.rs            # Core application state machine, persistent History & mode management
    ├── input.rs          # Direct key event normalization to keybinding tokens
    ├── config.rs         # JSON configuration loading & schema structures
    ├── search.rs         # fzf, ripgrep, and last-modified file search runners
    ├── actions.rs        # Filesystem operations, macOS pasteboard & Quick Look
    ├── bucket.rs         # Persistent bucket staging area manager
    └── ui.rs             # Ratatui view rendering for menus & prompts
```

## **Module Responsibilities**

* `main.rs`: Configures raw terminal modes via `crossterm`, sets up panic hooks, initializes async runtime, and teardown logic.  
* `app.rs`: Maintains state definitions (`AppMode`, `ActiveContext`, `History`, `PendingOperation`).  
* `input.rs`: Normalizes `crossterm` key events into keybinding tokens for direct lookup.  
* `config.rs`: Handles loading, parsing, and validation of user settings and state paths in `~/.config/mac-tui-fm/`.  
* `search.rs`: Wraps sub-process execution for `fzf` and `rg`, plus last-modified descending file listing for Recents.  
* `actions.rs`: Executes filesystem modifications, system commands via `std::process`, macOS `NSPasteboard` bindings, and `qlmanage` invocations.  
* `bucket.rs`: Controls persistent file staging lists (`~/.config/mac-tui-fm/bucket.json`) and staging directory symlinks.  
* `ui.rs`: Contains `ratatui` frame drawing logic for modal boxes, status lines, key binding hints, and file previews.

# **7\. Implementation To-Do List**

- [ ] **Phase 1: Project Scaffolding & Setup**  
  - [ ] Initialize Cargo project (`Cargo.toml`) with dependencies (`ratatui`, `crossterm`, `tokio`, `serde`, `serde_json`, `pasteboard`).  
  - [ ] Configure standard `Makefile` with targets for `build`, `install` (to `/usr/local/bin`), and `clean`.  
  - [ ] Establish system panic hooks in `src/main.rs` to ensure terminal raw state resets cleanly on failure.  
- [ ] **Phase 2: Configuration Engine**  
  - [ ] Implement `src/config.rs` structures to deserialize `keybindings.json` and `projects.json`.  
  - [ ] Add config file loader with fallback defaults created in `~/.config/mac-tui-fm/`.  
- [ ] **Phase 3: Input Parsing & Keybinding Dispatch**  
  - [ ] Normalize `crossterm` key events to keybinding tokens in `src/input.rs`.  
  - [ ] Map resolved single-key and modifier inputs directly to strongly typed domain action events.  
- [ ] **Phase 4: Search Engine Integrations**  
  - [ ] Implement async child process wrappers in `src/search.rs` for `fzf` calls.  
  - [ ] Implement `ripgrep` (`rg`) integration for text search scopes.  
  - [ ] Implement last-modified (newest descending) file discovery for **Recents** (`r`) across configured `search_roots`.  
  - [ ] Connect **Projects Search** (`p`) to filter paths exclusively based on configured project roots.  
- [ ] **Phase 5: Context State & Action Submenus**  
  - [ ] Build `ActiveFileContext` tracking inside `src/app.rs`.  
  - [ ] Implement submenus: **Copy Menu** (`c`), **Open Menu** (`o`), and **Path Menu** (`dir`).  
  - [ ] Implement basic filesystem operations (`m` Move, `x` Cut, `p` Paste).  
- [ ] **Phase 6: Native macOS Features Integration**  
  - [ ] Implement binary file copy to macOS system pasteboard for Finder cross-compatibility.  
  - [ ] Implement configurable editor launch (`e`) defaulting to Sublime Text (`subl`).  
  - [ ] Add terminal syntax highlighting for the split-pane search preview.  
- [ ] **Phase 7: Persistent State & Workflow Modifiers**  
  - [ ] Implement persistent **The Bucket** (`src/bucket.rs`) staging area system (`~/.config/mac-tui-fm/bucket.json`).  
  - [ ] Implement **Additionally** (`a`) workflow modifier to loop active context back into the Action Menu.  
  - [ ] Implement **Override** (`Ctrl+O`) modifier to dynamically swap active intents mid-selection.  
  - [ ] Add persistent ring buffer for **History** (`h`, `~/.config/mac-tui-fm/history.json`).  
- [ ] **Phase 8: Rendering & Polish**  
  - [ ] Complete `ratatui` UI layout components in `src/ui.rs` (Home Prompt, Search Overlay, History View, Action Modal, Status Line).  
  - [ ] End-to-end user workflow testing across search, history, recents, and bucket persistence.

# **8\. Development Workflow & Build Policy**

* **No Automatic Builds After Code Changes**: Do **not** automatically run `make build`, `make install`, or `cargo build --release` after every code edit. The user will trigger builds and installation manually when ready.
* **Fast Iteration**: During active development, prefer lightweight verification (`cargo check` or targeted `cargo test` when explicitly needed) and leave full binary builds (`cargo run`, `make build`, `make install`) to the user.

