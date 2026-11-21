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

The keybinding engine supports single keys, modifier combinations (`cmd + f`, `ctrl + p`), and modal sequential chord chains (e.g., `f` \$\\rightarrow\$ `f`).

```
{
  "settings": {
    "chord_timeout_ms": 1000,
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
      "pa": "action.paste",
      "h": "nav.home"
    }
  }
}
```

## **Projects Configuration**

Users define accessible project root paths in `~/.config/mac-tui-fm/projects.json`. Triggering Project Search mode (`p`) launches a fuzzy search (`fzf`) over the configured project list (matching by project `name` or `path`). Selecting a project sets the active search scope to that project's directory and presents the scoped command menu (`f`, `d`, `c`, `r`, `p`, `h`) restricted to that project root:

```
{
  "projects": [
    { "name": "System Engine", "path": "~/Developer/Rust/tui-file-manager" },
    { "name": "Web Services", "path": "~/Work/Backend/microservices" }
  ]
}
```

## **Chord Disambiguation (1-Second Timeout Rule)**

Modal key sequences frequently feature overlapping prefixes (e.g., `o` vs. `od`). The input engine resolves ambiguity using a non-blocking 1-second (1000ms) timer state machine:

```
[Key Input 'o'] ---> Start 1000ms Timer ---> Key 'd' pressed before timeout?
                          |                                   |
                          | (Timeout Expires)                 | (Yes)
                          v                                   v
                  Execute Action: 'o'                 Execute Action: 'od'
                  (Open Submenu)                      (Open Directory)
```

1. **Prefix Registration**: When an initial key (e.g., `o`) matches both a standalone command and the prefix of a multi-key chord, the input buffer captures `o` and spawns a non-blocking timeout task.  
2. **Execution Path A (Disambiguated Chord)**: If a valid secondary key (`d`) is pressed within 1000ms, the pending timer is canceled, the buffer resolves to `od`, and the target function executes immediately.  
3. **Execution Path B (Timeout Expiry)**: If no secondary key is pressed within 1000ms, the timer fires a flush event, resolving the buffer to `o` and executing its standalone command.

# **3\. Initial Menu (Home Screen)**

Upon launch, the application opens into a command palette style interface showing active session state and mnemonic modal entry points:

```
┌────────────────────────────────────────────────────────────────────────┐
│ macOS Modal TUI File Manager v0.1.0                                   │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   [f]   Find File          Fuzzy search files via fzf / ripgrep         │
│   [d]   Find Directory     Fuzzy search system directories             │
│   [c]   Find Content       Fuzzy search file contents via rg + fzf     │
│   [r]   Recents            Browse session file history                │
│   [p]   Projects           Search strictly within configured roots     │
│   [b]   Bucket             Open the session Bucket staging workspace   │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

* `f` (**Find File**): Opens fuzzy finder over all files in the active scope using `fzf`/`ripgrep`.  
* `d` (**Find Directory**): Filters strictly for directory paths.  
* `c` (**Find Content**): Prompts for directory scope selection from `settings.search_roots` (`1`–`9` to select an individual directory, or `0` to search within **all** specified directories). Once a scope is chosen, it waits for at least 3 characters of input before executing `ripgrep` (`rg`) with `fzf` to fuzzy-search file contents within that scope. Search results are displayed in a split layout with a **preview pane visible to the right of the results pane**.
  * **Results Navigation**: Strictly `Up`/`Down` arrow keys + `Enter` to select a single file (no `1`–`9` result pickers and no `Space` multi-selection across any fuzzy search mode: `f`, `d`, `c`, `p`).
  * **Preview Pane Controls**: `1` (press/hold) smooth-scrolls up, `2` (press/hold) smooth-scrolls down, `t` jumps to top, `b` jumps to bottom.  
* `r` (**Recents**): From the global Home Screen, displays up to 9 in-memory session history entries (`1`–`9`). When invoked inside a **Project Scope**, lists up to **9 most recently modified entries** within that project (sorted by last-modified timestamp, fuzzy-filterable via `fzf`, quick-selectable via `1`–`9`).  
* `p` (**Projects**): Fuzzy-searches the configured project list in `projects.json`. Selecting a project sets the active directory scope to that project root and re-displays the command menu (`f`, `d`, `c`, `r`, `p`, `h`) scoped to that project.  
* `b` (**Bucket**): Opens the session Bucket staging workspace.

# **4\. Post-Selection File Context & Action Submenus**

Selecting a file or directory entry from search (`f`, `d`, `c`) or `r` (**Recents**) updates the **Active Context** and transitions the UI directly to the **Action Menu**. (By contrast, selecting a project in `p` (**Projects**) sets the active search scope and presents the scoped command menu: `f`, `d`, `c`, `r`, `p`, `h`.)

```
Active File Context: /Users/dev/Projects/engine/src/main.rs
┌────────────────────────────────────────────────────────────────────────┐
│ ACTION MENU                                                            │
├────────────────────────────────────────────────────────────────────────┤
│  [m] Move        [c] Copy Menu      [x] Cut          [e] Edit          │
│  [b] Bucket      [o] Open Menu      [pa] Paste*      [h] Home          │
└────────────────────────────────────────────────────────────────────────┘
```
*\*Note: `[pa] Paste` is only shown when the active context is a **directory** (omitted when the active context is a file).*

## **Action Mapping Table**

| Chord | Target Action / Submenu | Description |
| :---- | :---- | :---- |
| `m` | **Move** | Opens a **strict** directory finder (`fzf --exact` / strict path matching, not loose fuzzy matching) scoped to configured directories (`settings.search_roots`) so new paths are not hijacked by opportunistic matches. If no existing directory strictly matches the typed path (e.g., `desktop/non-existent-dir`), pressing `Enter` creates it via `mkdir -p` and moves the file into it. |
| `c` | **Copy Submenu** | Opens specialized copy modes (Name, Content, Native File, Path). |
| `x` | **Cut** | Marks context for cut-and-paste relocation and immediately opens the **strict** directory finder (`fzf --exact` / strict path matching) scoped to `settings.search_roots`. If no existing directory strictly matches the typed path (e.g., `desktop/non-existent-dir`), pressing `Enter` creates it via `mkdir -p` and moves/pastes the file into it. |
| `e` | **Edit** | Opens the active file in the configured editor (default: Sublime Text / `subl`, configurable via `settings.editor` in JSON). |
| `b` | **Bucket** | Adds/stages the active file context into the session Bucket (deduplicating if already present) and returns to the Main Menu (Home Screen). |
| `o` | **Open Submenu** | Opens submenu: `(f)` **File** or `(d)` **Directory** (opens containing folder). |
| `pa` | **Paste** | Pastes current clipboard contents or pending cut/copy file context (shown only when active context is a directory). |
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

# **5\. Session State & Workflow Modifiers**

## **The Bucket**

The **Bucket** is a temporary, session-scoped virtual workspace and staging directory managed by the application.

* Acts as a scratchpad for files collected across disparate directories, displayed in a numbered `1`–`9` list where pressing `1`–`9` selects a staged entry.  
* Supports multi-selection via `Space` (toggle selection on the highlighted item) and `Up`/`Down` arrow key navigation, allowing batch actions across multiple selected files.  
* Supports staging files and acting on them directly from the Bucket interface.  
* Exposes native macOS pasteboard exports for pasting files or images directly into Finder or web apps (e.g., `Cmd+V` upload to Imgur).

### **Bucket Action Keybindings**

| Key | Action / Submenu | Description |
| :---- | :---- | :---- |
| `c` | **Copy Submenu** | Opens copy submenu:<br>• `(f)` **Copy File**: Copies native file/binary payload to `NSPasteboard` (e.g., `Cmd+V` into Finder or Imgur).<br>• `(c)` **Copy Contents**: Copies plaintext file contents (`.js`, `.txt`, `.md`, non-binary).<br>• `(d)` **Copy Dir Path**: Opens submenu with `(r)` Relative path and `(f)` Full (absolute) path. |
| `o` | **Open Submenu** | Opens submenu:<br>• `(f)` **Finder**: Reveals/opens in macOS Finder.<br>• `(o)` **Open File**: Opens file with default macOS application.<br>• `(d)` **Open Directory**: Opens the Bucket's temporary staging directory. |
| `a` | **Archive** | Clears the contents of the Bucket. |
| `h` | **Home** | Returns to the Home Screen. |

## **Workflow Modifiers**

### **Additionally (`a`)**

The `Additionally` key acts as a chaining modifier. When appended to an action execution:

1. The executed command processes immediately.  
2. The active file context is preserved in memory rather than cleared.  
3. The UI automatically re-opens the **Action Submenu** for subsequent operations on the same context without requiring re-selection.

### **Override (`o`)**

The `Override` key provides mid-operation intent modification. If an operation is currently pending (e.g., a file is marked for a **Cut** operation):

* Pressing `o` allows the user to switch the pending state to **Copy** (or vice versa) without aborting the selection flow or clearing the context.

### **Session Recents**

* Tracks all files, paths, and directories accessed or mutated during the active session (global Home Screen `r`).  
* When `r` is invoked inside a **Project Scope**, it instead queries the project directory for up to **9 entries sorted by last-modified time** (fuzzy-filterable via `fzf`).  
* Each accessed or mutated path (including both the original source path and the resulting destination path after a move) is recorded as its own distinct entry.  
* Maintained in an in-memory ring buffer displaying up to **9 items** (`1`–`9`) via `r`; pressing a number key `1`–`9` immediately selects the corresponding entry.  
* If a selected recent entry no longer exists on disk, the UI displays a **"File does not exist"** prompt and waits for follow-up input or `Esc`.

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
    ├── app.rs            # Core application state machine & mode management
    ├── input.rs          # Input event handling & 1s chord timeout parser
    ├── config.rs         # JSON configuration loading & schema structures
    ├── search.rs         # fzf and ripgrep async child process runners
    ├── actions.rs        # Filesystem operations, macOS pasteboard & Quick Look
    ├── bucket.rs         # Session bucket staging area manager
    └── ui.rs             # Ratatui view rendering for menus & prompts
```

## **Module Responsibilities**

* `main.rs`: Configures raw terminal modes via `crossterm`, sets up panic hooks, initializes async runtime, and teardown logic.  
* `app.rs`: Maintains state definitions (`AppMode`, `ActiveContext`, `SessionRecents`, `PendingOperation`).  
* `input.rs`: Implements the key chord parser and Tokio-backed timer task for resolving ambiguous 1-second timeout chains (`o` vs `od`).  
* `config.rs`: Handles loading, parsing, and validation of user settings from `~/.config/mac-tui-fm/`.  
* `search.rs`: Wraps sub-process execution for `fzf` and `rg` with stream redirection into the TUI widgets.  
* `actions.rs`: Executes filesystem modifications, system commands via `std::process`, macOS `NSPasteboard` bindings, and `qlmanage` invocations.  
* `bucket.rs`: Controls temporary file collection arrays and staging directory syncing.  
* `ui.rs`: Contains `ratatui` frame drawing logic for modal boxes, status lines, key binding hints, and file previews.

# **7\. Implementation To-Do List**

- [ ] **Phase 1: Project Scaffolding & Setup**  
  - [ ] Initialize Cargo project (`Cargo.toml`) with dependencies (`ratatui`, `crossterm`, `tokio`, `serde`, `serde_json`, `pasteboard`).  
  - [ ] Configure standard `Makefile` with targets for `build`, `install` (to `/usr/local/bin`), and `clean`.  
  - [ ] Establish system panic hooks in `src/main.rs` to ensure terminal raw state resets cleanly on failure.  
- [ ] **Phase 2: Configuration Engine**  
  - [ ] Implement `src/config.rs` structures to deserialize `keybindings.json` and `projects.json`.  
  - [ ] Add config file loader with fallback defaults created in `~/.config/mac-tui-fm/`.  
- [ ] **Phase 3: Input Parsing & 1-Second Chord Disambiguation**  
  - [ ] Build key event buffer in `src/input.rs`.  
  - [ ] Implement non-blocking 1-second (1000ms) timer for multi-key prefix detection (e.g., differentiating `o` / `od`).  
  - [ ] Map resolved inputs to strongly typed domain action events.  
- [ ] **Phase 4: Search Engine Integrations**  
  - [ ] Implement async child process wrappers in `src/search.rs` for `fzf` calls.  
  - [ ] Implement `ripgrep` (`rg`) integration for text search scopes.  
  - [ ] Connect **Projects Search** (`p`) to filter paths exclusively based on configured project roots.  
- [ ] **Phase 5: Context State & Action Submenus**  
  - [ ] Build `ActiveFileContext` tracking inside `src/app.rs`.  
  - [ ] Implement submenus: **Copy Menu** (`c`), **Open Menu** (`o`), and **Path Menu** (`dir`).  
  - [ ] Implement basic filesystem operations (`m` Move, `x` Cut, `pa` Paste).  
- [ ] **Phase 6: Native macOS Features Integration**  
  - [ ] Implement binary file copy to macOS system pasteboard for Finder cross-compatibility.  
  - [ ] Implement configurable editor launch (`e`) defaulting to Sublime Text (`subl`).  
  - [ ] Add terminal syntax highlighting for the split-pane search preview.  
- [ ] **Phase 7: Session Modifiers & Staging Systems**  
  - [ ] Implement **The Bucket** (`src/bucket.rs`) staging area system.  
  - [ ] Implement **Additionally** (`a`) workflow modifier to loop active context back into the Action Menu.  
  - [ ] Implement **Override** (`o`) modifier to dynamically swap active intents mid-selection.  
  - [ ] Add in-memory session history ring buffer for **Recents** (`r`).  
- [ ] **Phase 8: Rendering & Polish**  
  - [ ] Complete `ratatui` UI layout components in `src/ui.rs` (Home Prompt, Search Overlay, Action Modal, Status Line).  
  - [ ] End-to-end user workflow testing across complex chord combinations and timeout edge cases.

