use crate::actions;
use crate::bucket::Bucket;
use crate::config::{
    bucket_path, expand_tilde, history_path, resolve_target_dir, LoadedConfig, ProjectEntry,
};
use crate::input::key_event_to_token;
use crate::search::{self, SearchResult};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub const HISTORY_CAPACITY: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingOperation {
    Cut(PathBuf),
    Copy(PathBuf),
}

impl PendingOperation {
    pub fn label(&self) -> &'static str {
        match self {
            PendingOperation::Cut(_) => "CUT",
            PendingOperation::Copy(_) => "COPY",
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            PendingOperation::Cut(p) | PendingOperation::Copy(p) => p,
        }
    }

    pub fn toggle_override(&mut self) {
        *self = match self {
            PendingOperation::Cut(p) => PendingOperation::Copy(p.clone()),
            PendingOperation::Copy(p) => PendingOperation::Cut(p.clone()),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetDirIntent {
    Move,
    Cut,
    Copy,
}

impl TargetDirIntent {
    pub fn label(&self) -> &'static str {
        match self {
            TargetDirIntent::Move => "MOVE",
            TargetDirIntent::Cut => "CUT",
            TargetDirIntent::Copy => "COPY (Override)",
        }
    }

    pub fn toggle_override(&mut self) {
        *self = match self {
            TargetDirIntent::Cut | TargetDirIntent::Move => TargetDirIntent::Copy,
            TargetDirIntent::Copy => TargetDirIntent::Cut,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchKind {
    FindFile,
    FindDirectory,
    FindContent { roots: Vec<PathBuf> },
    Recents,
    Projects,
    StrictDirectoryTarget { intent: TargetDirIntent },
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub kind: SearchKind,
    pub query: String,
    pub all_candidates: Vec<String>,
    pub results: Vec<SearchResult>,
    pub selected_idx: usize,
    pub preview_mode: bool,
    pub preview_lines: Vec<String>,
    pub preview_path: Option<PathBuf>,
    pub preview_scroll: usize,
    pub preview_highlight_line: Option<usize>,
    pub loading: bool,
    pub search_gen: u64,
}

impl SearchState {
    pub fn new(kind: SearchKind, gen: u64) -> Self {
        Self {
            kind,
            query: String::new(),
            all_candidates: Vec::new(),
            results: Vec::new(),
            selected_idx: 0,
            preview_mode: false,
            preview_lines: Vec::new(),
            preview_path: None,
            preview_scroll: 0,
            preview_highlight_line: None,
            loading: true,
            search_gen: gen,
        }
    }

    pub fn is_strict_dir_target(&self) -> bool {
        matches!(self.kind, SearchKind::StrictDirectoryTarget { .. })
    }

    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_idx)
    }

    pub fn refresh_preview(&mut self) {
        let Some(res) = self.results.get(self.selected_idx).cloned() else {
            self.preview_path = None;
            self.preview_lines.clear();
            self.preview_scroll = 0;
            self.preview_highlight_line = None;
            return;
        };

        let needs_reload = self.preview_path.as_ref() != Some(&res.path);
        self.preview_highlight_line = res.line_number;

        if needs_reload {
            self.preview_path = Some(res.path.clone());
            self.preview_scroll = 0;
            self.preview_lines = load_preview_lines(&res.path);
        }

        if let Some(line_no) = res.line_number {
            self.preview_scroll = line_no.saturating_sub(5);
        }
    }

    pub fn scroll_preview_up(&mut self, lines: usize) {
        self.preview_scroll = self.preview_scroll.saturating_sub(lines);
    }

    pub fn scroll_preview_down(&mut self, lines: usize) {
        let max_scroll = self.preview_lines.len().saturating_sub(1);
        self.preview_scroll = (self.preview_scroll + lines).min(max_scroll);
    }

    pub fn scroll_preview_top(&mut self) {
        self.preview_scroll = 0;
    }

    pub fn scroll_preview_bottom(&mut self) {
        self.preview_scroll = self.preview_lines.len().saturating_sub(1);
    }
}

fn load_preview_lines(path: &Path) -> Vec<String> {
    if !path.exists() {
        return vec!["<Path does not exist>".to_string()];
    }
    if path.is_dir() {
        let mut lines = vec![format!("Directory: {}", path.display()), String::new()];
        if let Ok(entries) = fs::read_dir(path) {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|e| {
                    let is_d = e.path().is_dir();
                    let name = e.file_name().to_string_lossy().to_string();
                    if is_d {
                        format!("{}/", name)
                    } else {
                        name
                    }
                })
                .collect();
            names.sort();
            for n in names.into_iter().take(200) {
                lines.push(format!("  {}", n));
            }
        }
        return lines;
    }

    if !actions::is_plaintext_file(path) {
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        return vec![
            format!("Binary file: {}", path.display()),
            format!("Size: {} bytes", size),
            String::new(),
            "Preview not available for binary files.".to_string(),
        ];
    }

    match fs::read_to_string(path) {
        Ok(content) => content.lines().take(2000).map(String::from).collect(),
        Err(e) => vec![format!("<Unable to read file: {}>", e)],
    }
}

#[derive(Debug, Clone)]
pub struct HistoryState {
    pub entries: Vec<PathBuf>,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BucketSubmenu {
    Main,
    Copy,
    Open,
}

#[derive(Debug, Clone)]
pub enum AppMode {
    Home,
    ContentScopeSelect,
    Search(SearchState),
    History(HistoryState),
    FileNotFoundPrompt { path: PathBuf },
    ActionMenu,
    CopySubmenu,
    PathSubmenu { from_bucket: bool },
    OpenSubmenu,
    Bucket(BucketSubmenu),
}

#[derive(Debug)]
pub enum AsyncEvent {
    CandidatesLoaded {
        gen: u64,
        candidates: Vec<String>,
        initial_results: Vec<SearchResult>,
    },
    SearchResults {
        gen: u64,
        results: Vec<SearchResult>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryData {
    #[serde(default)]
    entries: Vec<PathBuf>,
}

/// Persistent ring buffer storing up to 9 recently accessed or mutated paths.
#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<PathBuf>,
    storage_path: Option<PathBuf>,
}

impl History {
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            entries: VecDeque::new(),
            storage_path: None,
        }
    }

    pub fn load(storage_path: PathBuf) -> Self {
        let mut entries = VecDeque::new();
        if let Ok(raw) = fs::read_to_string(&storage_path) {
            if let Ok(data) = serde_json::from_str::<HistoryData>(&raw) {
                for path in data.entries.into_iter().take(HISTORY_CAPACITY) {
                    if !entries.contains(&path) {
                        entries.push_back(path);
                    }
                }
            }
        }
        Self {
            entries,
            storage_path: Some(storage_path),
        }
    }

    fn save(&self) {
        let Some(ref path) = self.storage_path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data = HistoryData {
            entries: self.entries.iter().cloned().collect(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = fs::write(path, json);
        }
    }

    pub fn record(&mut self, path: PathBuf) {
        self.entries.retain(|p| p != &path);
        self.entries.push_front(path);
        while self.entries.len() > HISTORY_CAPACITY {
            self.entries.pop_back();
        }
        self.save();
    }

    pub fn list(&self) -> Vec<PathBuf> {
        self.entries.iter().cloned().collect()
    }
}

pub struct App {
    pub config: LoadedConfig,
    pub mode: AppMode,
    pub active_context: Option<PathBuf>,
    pub active_project: Option<ProjectEntry>,
    pub pending_operation: Option<PendingOperation>,
    pub additionally_active: bool,
    pub history: History,
    pub bucket: Bucket,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub async_tx: mpsc::UnboundedSender<AsyncEvent>,
    next_gen: u64,
}

impl App {
    pub fn new(config: LoadedConfig, async_tx: mpsc::UnboundedSender<AsyncEvent>) -> Self {
        Self {
            config,
            mode: AppMode::Home,
            active_context: None,
            active_project: None,
            pending_operation: None,
            additionally_active: false,
            history: History::load(history_path()),
            bucket: Bucket::load(bucket_path()),
            status_message: None,
            should_quit: false,
            async_tx,
            next_gen: 1,
        }
    }

    #[cfg(test)]
    pub fn new_in_memory(
        config: LoadedConfig,
        async_tx: mpsc::UnboundedSender<AsyncEvent>,
    ) -> Self {
        Self {
            config,
            mode: AppMode::Home,
            active_context: None,
            active_project: None,
            pending_operation: None,
            additionally_active: false,
            history: History::in_memory(),
            bucket: Bucket::new(),
            status_message: None,
            should_quit: false,
            async_tx,
            next_gen: 1,
        }
    }

    fn alloc_gen(&mut self) -> u64 {
        let g = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1);
        g
    }

    pub fn active_search_roots(&self) -> Vec<PathBuf> {
        if let Some(proj) = &self.active_project {
            vec![expand_tilde(&proj.path)]
        } else {
            self.config.expanded_search_roots()
        }
    }

    pub fn active_project_root(&self) -> Option<PathBuf> {
        self.active_project.as_ref().map(|p| expand_tilde(&p.path))
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Handles asynchronous search/listing results.
    pub fn handle_async_event(&mut self, ev: AsyncEvent) {
        match ev {
            AsyncEvent::CandidatesLoaded {
                gen,
                candidates,
                initial_results,
            } => {
                if let AppMode::Search(ref mut st) = self.mode {
                    if st.search_gen == gen {
                        st.all_candidates = candidates;
                        st.loading = false;
                        if st.query.is_empty() {
                            st.results = initial_results;
                            st.selected_idx = 0;
                            st.refresh_preview();
                        } else {
                            // User already started typing while candidates were loading
                            self.spawn_filter_for_current_search();
                        }
                    }
                }
            }
            AsyncEvent::SearchResults { gen, results } => {
                if let AppMode::Search(ref mut st) = self.mode {
                    if st.search_gen == gen {
                        st.results = results;
                        st.selected_idx = 0;
                        st.loading = false;
                        st.refresh_preview();
                    }
                }
            }
        }
    }

    /// Main keyboard event dispatcher.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Global Ctrl+C always exits cleanly
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match &self.mode {
            AppMode::Home => self.handle_key_home(key),
            AppMode::ContentScopeSelect => self.handle_key_content_scope(key),
            AppMode::Search(_) => self.handle_key_search(key),
            AppMode::History(_) => self.handle_key_history(key),
            AppMode::FileNotFoundPrompt { .. } => self.handle_key_file_not_found(key),
            AppMode::ActionMenu => self.handle_key_action_menu(key),
            AppMode::CopySubmenu => self.handle_key_copy_submenu(key),
            AppMode::PathSubmenu { .. } => self.handle_key_path_submenu(key),
            AppMode::OpenSubmenu => self.handle_key_open_submenu(key),
            AppMode::Bucket(_) => self.handle_key_bucket(key),
        }
    }

    fn handle_key_home(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => {
                self.should_quit = true;
                return;
            }
            KeyCode::Esc => {
                if self.active_project.is_some() {
                    self.active_project = None;
                    self.set_status("Cleared project scope — back to global Home");
                }
                return;
            }
            _ => {}
        }

        if let Some(token) = key_event_to_token(key) {
            if let Some(cmd) = self
                .config
                .app
                .keybindings
                .home_screen
                .get(&token)
                .cloned()
            {
                self.execute_command(&cmd);
            }
        }
    }

    fn handle_key_content_scope(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Home;
                self.set_status("Cancelled Find Content");
            }
            KeyCode::Char('0') => {
                let roots = self.active_search_roots();
                self.start_search(SearchKind::FindContent { roots });
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                let roots = self.active_search_roots();
                if let Some(chosen) = roots.get(idx) {
                    self.start_search(SearchKind::FindContent {
                        roots: vec![chosen.clone()],
                    });
                } else {
                    self.set_status("Invalid scope selection");
                }
            }
            _ => {}
        }
    }

    fn handle_key_search(&mut self, key: KeyEvent) {
        let is_strict = matches!(&self.mode, AppMode::Search(s) if s.is_strict_dir_target());

        // Ctrl+O in strict directory target toggles Override (Cut/Move <-> Copy)
        if is_strict
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('o')
        {
            if let AppMode::Search(ref mut st) = self.mode {
                if let SearchKind::StrictDirectoryTarget { ref mut intent } = st.kind {
                    intent.toggle_override();
                    let label = intent.label();
                    if let Some(ref mut pending) = self.pending_operation {
                        pending.toggle_override();
                    }
                    self.set_status(format!("Override active — mode switched to {}", label));
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => {
                let was_preview_mode =
                    matches!(&self.mode, AppMode::Search(s) if s.preview_mode);
                if was_preview_mode {
                    if let AppMode::Search(ref mut st) = self.mode {
                        st.preview_mode = false;
                    }
                    return;
                }
                if is_strict {
                    self.mode = AppMode::ActionMenu;
                    self.set_status("Cancelled directory selection");
                } else {
                    self.mode = AppMode::Home;
                }
                return;
            }
            KeyCode::Tab => {
                if let AppMode::Search(ref mut st) = self.mode {
                    st.preview_mode = !st.preview_mode;
                }
                return;
            }
            KeyCode::Up => {
                if let AppMode::Search(ref mut st) = self.mode {
                    if !st.results.is_empty() {
                        if st.selected_idx > 0 {
                            st.selected_idx -= 1;
                        } else {
                            st.selected_idx = st.results.len() - 1;
                        }
                        st.refresh_preview();
                    }
                }
                return;
            }
            KeyCode::Down => {
                if let AppMode::Search(ref mut st) = self.mode {
                    if !st.results.is_empty() {
                        st.selected_idx = (st.selected_idx + 1) % st.results.len();
                        st.refresh_preview();
                    }
                }
                return;
            }
            KeyCode::Enter => {
                self.confirm_search_selection();
                return;
            }
            _ => {}
        }

        // Preview pane controls when preview_mode is toggled on:
        // `1` smooth-scroll up, `2` smooth-scroll down, `t` top, `b` bottom.
        let in_preview_mode = matches!(&self.mode, AppMode::Search(s) if s.preview_mode);
        if in_preview_mode {
            if let AppMode::Search(ref mut st) = self.mode {
                match key.code {
                    KeyCode::Char('1') => st.scroll_preview_up(2),
                    KeyCode::Char('2') => st.scroll_preview_down(2),
                    KeyCode::Char('t') => st.scroll_preview_top(),
                    KeyCode::Char('b') => st.scroll_preview_bottom(),
                    KeyCode::Char('i') | KeyCode::Char('/') => st.preview_mode = false,
                    _ => {}
                }
            }
            return;
        }

        // Query input mode (strictly no 1-9 result pickers and no Space multi-select)
        let mut query_changed = false;
        if let AppMode::Search(ref mut st) = self.mode {
            match key.code {
                KeyCode::Backspace => {
                    if st.query.pop().is_some() {
                        query_changed = true;
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    st.query.push(c);
                    query_changed = true;
                }
                _ => {}
            }
        }

        if query_changed {
            self.spawn_filter_for_current_search();
        }
    }

    fn confirm_search_selection(&mut self) {
        let state = match &self.mode {
            AppMode::Search(s) => s.clone(),
            _ => return,
        };

        match state.kind {
            SearchKind::Projects => {
                if let Some(res) = state.selected_result() {
                    let name = res
                        .project_name
                        .clone()
                        .unwrap_or_else(|| res.path.display().to_string());
                    self.active_project = Some(ProjectEntry {
                        name: name.clone(),
                        path: res.path.display().to_string(),
                    });
                    self.mode = AppMode::Home;
                    self.set_status(format!("Project scope set to: {}", name));
                }
            }
            SearchKind::FindFile
            | SearchKind::FindDirectory
            | SearchKind::FindContent { .. }
            | SearchKind::Recents => {
                if let Some(res) = state.selected_result() {
                    let path = res.path.clone();
                    self.select_active_context(path);
                }
            }
            SearchKind::StrictDirectoryTarget { ref intent } => {
                let intent = intent.clone();
                let target_dir = if let Some(res) = state.selected_result() {
                    res.path.clone()
                } else if !state.query.trim().is_empty() {
                    let roots = self.active_search_roots();
                    resolve_target_dir(&state.query, &roots)
                } else {
                    self.set_status("Please type or select a target directory");
                    return;
                };

                self.execute_target_dir_transfer(target_dir, intent);
            }
        }
    }

    fn execute_target_dir_transfer(&mut self, target_dir: PathBuf, intent: TargetDirIntent) {
        let Some(src) = self.active_context.clone() else {
            self.mode = AppMode::Home;
            return;
        };

        let result = match intent {
            TargetDirIntent::Move | TargetDirIntent::Cut => {
                actions::move_into_dir(&src, &target_dir)
            }
            TargetDirIntent::Copy => actions::copy_into_dir(&src, &target_dir),
        };

        match result {
            Ok(dest) => {
                // Record both original source path and resulting destination path as distinct entries
                self.history.record(src.clone());
                self.history.record(dest.clone());
                self.pending_operation = None;
                let verb = match intent {
                    TargetDirIntent::Move => "Moved",
                    TargetDirIntent::Cut => "Cut & moved",
                    TargetDirIntent::Copy => "Copied (Override)",
                };
                self.active_context = Some(dest.clone());
                self.set_status(format!("{} to {}", verb, dest.display()));
                self.finish_action();
            }
            Err(e) => {
                self.mode = AppMode::ActionMenu;
                self.set_status(format!("Error: {}", e));
            }
        }
    }

    fn handle_key_history(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Home;
                return;
            }
            KeyCode::Up => {
                if let AppMode::History(ref mut st) = self.mode {
                    if !st.entries.is_empty() {
                        if st.cursor > 0 {
                            st.cursor -= 1;
                        } else {
                            st.cursor = st.entries.len() - 1;
                        }
                    }
                }
                return;
            }
            KeyCode::Down => {
                if let AppMode::History(ref mut st) = self.mode {
                    if !st.entries.is_empty() {
                        st.cursor = (st.cursor + 1) % st.entries.len();
                    }
                }
                return;
            }
            KeyCode::Enter => {
                let chosen = match &self.mode {
                    AppMode::History(st) => st.entries.get(st.cursor).cloned(),
                    _ => None,
                };
                if let Some(p) = chosen {
                    self.open_history_entry(p);
                }
                return;
            }
            _ => {}
        }

        // Quick-select 1..=9
        if let KeyCode::Char(c @ '1'..='9') = key.code {
            let idx = (c as u8 - b'1') as usize;
            let chosen = match &self.mode {
                AppMode::History(st) => st.entries.get(idx).cloned(),
                _ => None,
            };
            if let Some(p) = chosen {
                self.open_history_entry(p);
            }
        }
    }

    fn open_history_entry(&mut self, path: PathBuf) {
        if !path.exists() {
            self.mode = AppMode::FileNotFoundPrompt { path };
        } else {
            self.select_active_context(path);
        }
    }

    fn handle_key_file_not_found(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Home;
            }
            KeyCode::Char('h') => {
                self.open_history();
            }
            KeyCode::Char('r') => {
                self.start_search(SearchKind::Recents);
            }
            _ => {
                self.mode = AppMode::Home;
            }
        }
    }

    fn handle_key_action_menu(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.active_context = None;
                self.additionally_active = false;
                self.mode = AppMode::Home;
                return;
            }
            // Ctrl+O or Override key when a pending operation exists
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(ref mut pending) = self.pending_operation {
                    pending.toggle_override();
                    let label = pending.label();
                    self.set_status(format!("Override: pending operation switched to {}", label));
                } else {
                    self.set_status("No pending Cut/Copy operation to override");
                }
                return;
            }
            _ => {}
        }

        let is_dir = self
            .active_context
            .as_ref()
            .map(|p| p.is_dir())
            .unwrap_or(false);

        let mut bindings: HashMap<String, String> =
            self.config.app.keybindings.action_menu.clone();

        // Remove paste binding when active context is not a directory
        if !is_dir {
            bindings.retain(|_, v| v != "action.paste");
        }

        // Add `a` for Additionally modifier toggle
        bindings
            .entry("a".into())
            .or_insert_with(|| "modifier.additionally".into());

        if let Some(token) = key_event_to_token(key) {
            if let Some(cmd) = bindings.get(&token).cloned() {
                self.execute_command(&cmd);
            }
        }
    }

    fn handle_key_copy_submenu(&mut self, key: KeyEvent) {
        let Some(ctx) = self.active_context.clone() else {
            self.mode = AppMode::Home;
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::ActionMenu;
            }
            KeyCode::Char('n') => match actions::copy_file_name(&ctx) {
                Ok(name) => {
                    self.set_status(format!("Copied filename: {}", name));
                    self.finish_action();
                }
                Err(e) => {
                    self.set_status(format!("Copy failed: {}", e));
                    self.mode = AppMode::ActionMenu;
                }
            },
            KeyCode::Char('c') => match actions::copy_file_contents(&[ctx]) {
                Ok(()) => {
                    self.set_status("Copied file contents to clipboard");
                    self.finish_action();
                }
                Err(e) => {
                    self.set_status(format!("Copy contents failed: {}", e));
                    self.mode = AppMode::ActionMenu;
                }
            },
            KeyCode::Char('f') => match actions::copy_files_to_pasteboard(&[ctx.clone()]) {
                Ok(()) => {
                    self.pending_operation = Some(PendingOperation::Copy(ctx));
                    self.set_status("Copied native file to macOS Pasteboard");
                    self.finish_action();
                }
                Err(e) => {
                    self.set_status(format!("Pasteboard copy failed: {}", e));
                    self.mode = AppMode::ActionMenu;
                }
            },
            KeyCode::Char('d') => {
                self.mode = AppMode::PathSubmenu { from_bucket: false };
            }
            KeyCode::Char('a') => {
                self.additionally_active = !self.additionally_active;
                let state = if self.additionally_active { "ON" } else { "OFF" };
                self.set_status(format!("Additionally modifier: {}", state));
            }
            _ => {}
        }
    }

    fn handle_key_path_submenu(&mut self, key: KeyEvent) {
        let from_bucket = matches!(self.mode, AppMode::PathSubmenu { from_bucket: true });
        let targets = if from_bucket {
            self.bucket.effective_targets()
        } else {
            self.active_context.clone().into_iter().collect()
        };

        match key.code {
            KeyCode::Esc => {
                self.mode = if from_bucket {
                    AppMode::Bucket(BucketSubmenu::Copy)
                } else {
                    AppMode::CopySubmenu
                };
            }
            KeyCode::Char('r') => {
                let base = self.active_project_root();
                match actions::copy_relative_paths(&targets, base.as_deref()) {
                    Ok(s) => {
                        self.set_status(format!("Copied relative path: {}", s.replace('\n', ", ")));
                        if from_bucket {
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        } else {
                            self.finish_action();
                        }
                    }
                    Err(e) => {
                        self.set_status(format!("Error: {}", e));
                    }
                }
            }
            KeyCode::Char('a') | KeyCode::Char('f') => {
                match actions::copy_absolute_paths(&targets) {
                    Ok(s) => {
                        self.set_status(format!("Copied absolute path: {}", s.replace('\n', ", ")));
                        if from_bucket {
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        } else {
                            self.finish_action();
                        }
                    }
                    Err(e) => {
                        self.set_status(format!("Error: {}", e));
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_key_open_submenu(&mut self, key: KeyEvent) {
        let Some(ctx) = self.active_context.clone() else {
            self.mode = AppMode::Home;
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::ActionMenu;
            }
            KeyCode::Char('f') => {
                let editor = self.config.app.settings.editor.clone();
                match actions::open_file_smart(&ctx, &editor) {
                    Ok(()) => {
                        self.set_status(format!("Opened file: {}", ctx.display()));
                        self.finish_action();
                    }
                    Err(e) => {
                        self.set_status(format!("Open failed: {}", e));
                        self.mode = AppMode::ActionMenu;
                    }
                }
            }
            KeyCode::Char('d') => match actions::open_directory(&ctx) {
                Ok(()) => {
                    self.set_status("Opened containing directory");
                    self.finish_action();
                }
                Err(e) => {
                    self.set_status(format!("Open dir failed: {}", e));
                    self.mode = AppMode::ActionMenu;
                }
            },
            KeyCode::Char('q') => match actions::quick_look(&ctx) {
                Ok(()) => {
                    self.set_status("Launched Quick Look");
                    self.finish_action();
                }
                Err(e) => {
                    self.set_status(format!("Quick Look failed: {}", e));
                    self.mode = AppMode::ActionMenu;
                }
            },
            _ => {}
        }
    }

    fn handle_key_bucket(&mut self, key: KeyEvent) {
        let submenu = match &self.mode {
            AppMode::Bucket(sub) => sub.clone(),
            _ => return,
        };

        match submenu {
            BucketSubmenu::Main => match key.code {
                KeyCode::Esc | KeyCode::Char('h') => {
                    self.mode = AppMode::Home;
                }
                KeyCode::Up => self.bucket.move_up(),
                KeyCode::Down => self.bucket.move_down(),
                KeyCode::Char(' ') => self.bucket.toggle_select(),
                KeyCode::Char(c @ '1'..='9') => {
                    let num = (c as u8 - b'0') as usize;
                    self.bucket.select_number(num);
                }
                KeyCode::Char('c') => {
                    if self.bucket.is_empty() {
                        self.set_status("Bucket is empty");
                    } else {
                        self.mode = AppMode::Bucket(BucketSubmenu::Copy);
                    }
                }
                KeyCode::Char('o') => {
                    self.mode = AppMode::Bucket(BucketSubmenu::Open);
                }
                KeyCode::Char('a') => {
                    self.bucket.archive();
                    self.set_status("Archived (cleared) Bucket");
                }
                _ => {}
            },
            BucketSubmenu::Copy => {
                let targets = self.bucket.effective_targets();
                match key.code {
                    KeyCode::Esc => {
                        self.mode = AppMode::Bucket(BucketSubmenu::Main);
                    }
                    KeyCode::Char('f') => match actions::copy_files_to_pasteboard(&targets) {
                        Ok(()) => {
                            self.pending_operation = None;
                            self.set_status(format!(
                                "Copied {} file(s) to macOS Pasteboard",
                                targets.len()
                            ));
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                        Err(e) => {
                            self.set_status(format!("Pasteboard copy failed: {}", e));
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                    },
                    KeyCode::Char('c') => match actions::copy_file_contents(&targets) {
                        Ok(()) => {
                            self.set_status("Copied plaintext contents to clipboard");
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                        Err(e) => {
                            self.set_status(format!("Copy contents failed: {}", e));
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                    },
                    KeyCode::Char('d') => {
                        self.mode = AppMode::PathSubmenu { from_bucket: true };
                    }
                    _ => {}
                }
            }
            BucketSubmenu::Open => {
                let targets = self.bucket.effective_targets();
                match key.code {
                    KeyCode::Esc => {
                        self.mode = AppMode::Bucket(BucketSubmenu::Main);
                    }
                    KeyCode::Char('f') => match actions::reveal_in_finder(&targets) {
                        Ok(()) => {
                            self.set_status("Revealed in Finder");
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                        Err(e) => {
                            self.set_status(format!("Finder reveal failed: {}", e));
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                    },
                    KeyCode::Char('o') => match actions::open_with_default_app(&targets) {
                        Ok(()) => {
                            self.set_status("Opened with default macOS application");
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                        Err(e) => {
                            self.set_status(format!("Open failed: {}", e));
                            self.mode = AppMode::Bucket(BucketSubmenu::Main);
                        }
                    },
                    KeyCode::Char('d') => {
                        let staging = self.bucket.staging_dir().to_path_buf();
                        match actions::open_directory(&staging) {
                            Ok(()) => {
                                self.set_status(format!(
                                    "Opened Bucket staging dir: {}",
                                    staging.display()
                                ));
                                self.mode = AppMode::Bucket(BucketSubmenu::Main);
                            }
                            Err(e) => {
                                self.set_status(format!("Open staging dir failed: {}", e));
                                self.mode = AppMode::Bucket(BucketSubmenu::Main);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Executes a resolved domain command from keybinding configuration.
    pub fn execute_command(&mut self, cmd: &str) {
        match cmd {
            "search.find_file" => self.start_search(SearchKind::FindFile),
            "search.find_directory" => self.start_search(SearchKind::FindDirectory),
            "search.find_content" => {
                if let Some(proj) = &self.active_project {
                    let root = expand_tilde(&proj.path);
                    self.start_search(SearchKind::FindContent { roots: vec![root] });
                } else {
                    self.mode = AppMode::ContentScopeSelect;
                }
            }
            "search.recents" => self.start_search(SearchKind::Recents),
            "search.history" => self.open_history(),
            "search.projects" => self.start_search(SearchKind::Projects),
            "bucket.open" => {
                self.mode = AppMode::Bucket(BucketSubmenu::Main);
            }
            "nav.clear_project" => {
                self.active_project = None;
                self.mode = AppMode::Home;
                self.set_status("Returned to global Home Screen");
            }
            "nav.home" => {
                self.active_context = None;
                self.additionally_active = false;
                self.mode = AppMode::Home;
            }
            "action.move" => {
                self.start_search(SearchKind::StrictDirectoryTarget {
                    intent: TargetDirIntent::Move,
                });
            }
            "action.cut" => {
                if let Some(ctx) = self.active_context.clone() {
                    self.pending_operation = Some(PendingOperation::Cut(ctx));
                    self.start_search(SearchKind::StrictDirectoryTarget {
                        intent: TargetDirIntent::Cut,
                    });
                }
            }
            "submenu.copy" => {
                self.mode = AppMode::CopySubmenu;
            }
            "submenu.open" => {
                self.mode = AppMode::OpenSubmenu;
            }
            "action.edit" => {
                if let Some(ctx) = self.active_context.clone() {
                    let editor = self.config.app.settings.editor.clone();
                    match actions::open_in_editor(&ctx, &editor) {
                        Ok(()) => {
                            self.set_status(format!("Opened in {}: {}", editor, ctx.display()));
                            self.finish_action();
                        }
                        Err(e) => self.set_status(format!("Edit failed: {}", e)),
                    }
                }
            }
            "action.bucket" => {
                if let Some(ctx) = self.active_context.clone() {
                    let added = self.bucket.add(ctx.clone());
                    if added {
                        self.set_status(format!(
                            "Staged in Bucket ({} total): {}",
                            self.bucket.len(),
                            ctx.display()
                        ));
                    } else {
                        self.set_status("Already staged in Bucket (deduplicated)");
                    }
                    self.finish_action();
                }
            }
            "action.paste" => {
                self.execute_paste_into_active_dir();
            }
            "modifier.additionally" => {
                self.additionally_active = !self.additionally_active;
                let state = if self.additionally_active { "ON" } else { "OFF" };
                self.set_status(format!(
                    "Additionally modifier: {} (preserves context & loops Action Menu)",
                    state
                ));
            }
            _ => {}
        }
    }

    fn execute_paste_into_active_dir(&mut self) {
        let Some(dest_dir) = self.active_context.clone() else {
            return;
        };
        if !dest_dir.is_dir() {
            self.set_status("Paste is only available when active context is a directory");
            return;
        }

        if let Some(pending) = self.pending_operation.clone() {
            let res = match &pending {
                PendingOperation::Cut(src) => actions::move_into_dir(src, &dest_dir),
                PendingOperation::Copy(src) => actions::copy_into_dir(src, &dest_dir),
            };
            match res {
                Ok(new_path) => {
                    self.history.record(pending.path().to_path_buf());
                    self.history.record(new_path.clone());
                    if matches!(pending, PendingOperation::Cut(_)) {
                        self.pending_operation = None;
                    }
                    self.set_status(format!("Pasted into {}", new_path.display()));
                    self.finish_action();
                }
                Err(e) => self.set_status(format!("Paste failed: {}", e)),
            }
        } else {
            // Check macOS NSPasteboard for copied file URLs first (Finder Cmd+C, Bucket Copy File, etc.)
            if let Ok(pb_files) = actions::read_files_from_pasteboard() {
                if !pb_files.is_empty() {
                    let mut pasted_count = 0usize;
                    let mut last_dest = None;
                    for src in &pb_files {
                        match actions::copy_into_dir(src, &dest_dir) {
                            Ok(new_path) => {
                                self.history.record(src.clone());
                                self.history.record(new_path.clone());
                                last_dest = Some(new_path);
                                pasted_count += 1;
                            }
                            Err(e) => {
                                self.set_status(format!("Paste failed: {}", e));
                                return;
                            }
                        }
                    }
                    if pasted_count == 1 {
                        if let Some(d) = last_dest {
                            self.set_status(format!("Pasted into {}", d.display()));
                        }
                    } else {
                        self.set_status(format!(
                            "Pasted {} file(s) into {}",
                            pasted_count,
                            dest_dir.display()
                        ));
                    }
                    self.finish_action();
                    return;
                }
            }

            // Fallback: paste plain clipboard text into a new file in dest_dir
            match actions::read_text_from_clipboard() {
                Ok(text) if !text.is_empty() => {
                    let dest_file = dest_dir.join("pasted_clipboard.txt");
                    match fs::write(&dest_file, text) {
                        Ok(()) => {
                            self.history.record(dest_file.clone());
                            self.set_status(format!(
                                "Pasted clipboard text to {}",
                                dest_file.display()
                            ));
                            self.finish_action();
                        }
                        Err(e) => self.set_status(format!("Paste failed: {}", e)),
                    }
                }
                _ => self.set_status("Nothing pending to paste and clipboard is empty"),
            }
        }
    }

    /// Transitions after an action executes, honoring the `Additionally` workflow modifier.
    fn finish_action(&mut self) {
        if self.additionally_active {
            // Preserve active context and loop straight back to Action Menu
            self.additionally_active = false;
            self.mode = AppMode::ActionMenu;
        } else {
            self.active_context = None;
            self.mode = AppMode::Home;
        }
    }

    fn select_active_context(&mut self, path: PathBuf) {
        self.history.record(path.clone());
        self.active_context = Some(path);
        self.mode = AppMode::ActionMenu;
    }

    fn open_history(&mut self) {
        let entries = self.history.list();
        self.mode = AppMode::History(HistoryState { entries, cursor: 0 });
    }

    fn start_search(&mut self, kind: SearchKind) {
        let gen = self.alloc_gen();
        let show_hidden = self.config.app.settings.show_hidden_files;
        let roots = self.active_search_roots();
        let projects = self.config.projects.projects.clone();
        let tx = self.async_tx.clone();

        let mut st = SearchState::new(kind.clone(), gen);

        match kind {
            SearchKind::FindContent { .. } => {
                // Content search waits for >= 3 characters before running rg
                st.loading = false;
                self.mode = AppMode::Search(st);
            }
            SearchKind::Projects => {
                let candidates: Vec<String> = projects
                    .iter()
                    .map(|p| format!("{} :: {}", p.name, expand_tilde(&p.path).display()))
                    .collect();
                let initial_results: Vec<SearchResult> = projects
                    .iter()
                    .map(|p| {
                        let expanded = expand_tilde(&p.path);
                        SearchResult {
                            path: expanded,
                            line_number: None,
                            snippet: None,
                            project_name: Some(p.name.clone()),
                        }
                    })
                    .collect();
                st.all_candidates = candidates;
                st.results = initial_results;
                st.loading = false;
                st.refresh_preview();
                self.mode = AppMode::Search(st);
            }
            SearchKind::FindFile => {
                self.mode = AppMode::Search(st);
                tokio::spawn(async move {
                    let files = search::list_files(&roots, show_hidden)
                        .await
                        .unwrap_or_default();
                    let candidates: Vec<String> =
                        files.iter().map(|p| p.display().to_string()).collect();
                    let initial_results: Vec<SearchResult> = files
                        .into_iter()
                        .take(200)
                        .map(|p| SearchResult {
                            path: p,
                            line_number: None,
                            snippet: None,
                            project_name: None,
                        })
                        .collect();
                    let _ = tx.send(AsyncEvent::CandidatesLoaded {
                        gen,
                        candidates,
                        initial_results,
                    });
                });
            }
            SearchKind::Recents => {
                self.mode = AppMode::Search(st);
                tokio::spawn(async move {
                    let files = search::recent_modified_files(&roots, show_hidden)
                        .await
                        .unwrap_or_default();
                    let candidates: Vec<String> =
                        files.iter().map(|p| p.display().to_string()).collect();
                    let initial_results: Vec<SearchResult> = files
                        .into_iter()
                        .take(200)
                        .map(|p| SearchResult {
                            path: p,
                            line_number: None,
                            snippet: None,
                            project_name: None,
                        })
                        .collect();
                    let _ = tx.send(AsyncEvent::CandidatesLoaded {
                        gen,
                        candidates,
                        initial_results,
                    });
                });
            }
            SearchKind::FindDirectory | SearchKind::StrictDirectoryTarget { .. } => {
                self.mode = AppMode::Search(st);
                tokio::spawn(async move {
                    let dirs = search::list_directories(&roots, show_hidden)
                        .await
                        .unwrap_or_default();
                    let candidates: Vec<String> =
                        dirs.iter().map(|p| p.display().to_string()).collect();
                    let initial_results: Vec<SearchResult> = dirs
                        .into_iter()
                        .take(200)
                        .map(|p| SearchResult {
                            path: p,
                            line_number: None,
                            snippet: None,
                            project_name: None,
                        })
                        .collect();
                    let _ = tx.send(AsyncEvent::CandidatesLoaded {
                        gen,
                        candidates,
                        initial_results,
                    });
                });
            }
        }
    }

    fn spawn_filter_for_current_search(&mut self) {
        let gen = self.alloc_gen();
        let show_hidden = self.config.app.settings.show_hidden_files;
        let tx = self.async_tx.clone();

        let (kind, query, candidates) = match &mut self.mode {
            AppMode::Search(st) => {
                st.search_gen = gen;
                (st.kind.clone(), st.query.clone(), st.all_candidates.clone())
            }
            _ => return,
        };

        match kind {
            SearchKind::FindContent { roots } => {
                if query.trim().chars().count() < 3 {
                    if let AppMode::Search(ref mut st) = self.mode {
                        st.results.clear();
                        st.selected_idx = 0;
                        st.loading = false;
                        st.refresh_preview();
                    }
                    return;
                }
                if let AppMode::Search(ref mut st) = self.mode {
                    st.loading = true;
                }
                tokio::spawn(async move {
                    let results = search::search_content(&roots, &query, show_hidden)
                        .await
                        .unwrap_or_default();
                    let _ = tx.send(AsyncEvent::SearchResults { gen, results });
                });
            }
            SearchKind::Projects => {
                let projects = self.config.projects.projects.clone();
                if let AppMode::Search(ref mut st) = self.mode {
                    st.loading = true;
                }
                tokio::spawn(async move {
                    let matched = search::fzf_filter(&candidates, &query, false, false)
                        .await
                        .unwrap_or_default();
                    let results: Vec<SearchResult> = matched
                        .into_iter()
                        .filter_map(|line| {
                            let (name, path_str) = line.split_once(" :: ")?;
                            let path = PathBuf::from(path_str);
                            let _ = &projects;
                            Some(SearchResult {
                                path,
                                line_number: None,
                                snippet: None,
                                project_name: Some(name.to_string()),
                            })
                        })
                        .collect();
                    let _ = tx.send(AsyncEvent::SearchResults { gen, results });
                });
            }
            SearchKind::FindFile
            | SearchKind::FindDirectory
            | SearchKind::Recents
            | SearchKind::StrictDirectoryTarget { .. } => {
                let exact = matches!(kind, SearchKind::StrictDirectoryTarget { .. });
                let no_sort = matches!(kind, SearchKind::Recents);
                if let AppMode::Search(ref mut st) = self.mode {
                    st.loading = true;
                }
                tokio::spawn(async move {
                    let matched = search::fzf_filter(&candidates, &query, exact, no_sort)
                        .await
                        .unwrap_or_default();
                    let results: Vec<SearchResult> = matched
                        .into_iter()
                        .map(|line| SearchResult {
                            path: PathBuf::from(line),
                            line_number: None,
                            snippet: None,
                            project_name: None,
                        })
                        .collect();
                    let _ = tx.send(AsyncEvent::SearchResults { gen, results });
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_ring_buffer() {
        let mut history = History::in_memory();
        for i in 0..12 {
            history.record(PathBuf::from(format!("/tmp/file_{}.txt", i)));
        }
        let list = history.list();
        assert_eq!(list.len(), 9);
        assert_eq!(list[0], PathBuf::from("/tmp/file_11.txt"));
        assert_eq!(list[8], PathBuf::from("/tmp/file_3.txt"));
    }

    #[test]
    fn test_history_persists_across_sessions() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "mac-tui-fm-history-persist-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();
        let storage = tmp_dir.join("history.json");

        {
            let mut history = History::load(storage.clone());
            history.record(PathBuf::from("/tmp/first.txt"));
            history.record(PathBuf::from("/tmp/second.txt"));
        }

        let reloaded = History::load(storage.clone());
        assert_eq!(
            reloaded.list(),
            vec![
                PathBuf::from("/tmp/second.txt"),
                PathBuf::from("/tmp/first.txt")
            ]
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_override_toggles_pending_operation() {
        let mut op = PendingOperation::Cut(PathBuf::from("/tmp/a.rs"));
        assert_eq!(op.label(), "CUT");
        op.toggle_override();
        assert_eq!(op.label(), "COPY");
        op.toggle_override();
        assert_eq!(op.label(), "CUT");
    }

    #[tokio::test]
    async fn test_additionally_preserves_context_and_reopens_action_menu() {
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let cfg = LoadedConfig {
            app: Default::default(),
            projects: Default::default(),
        };
        let mut app = App::new_in_memory(cfg, a_tx);
        let path = PathBuf::from("/tmp/example_additionally.txt");
        app.select_active_context(path.clone());
        assert!(matches!(app.mode, AppMode::ActionMenu));

        // Toggle Additionally modifier
        app.execute_command("modifier.additionally");
        assert!(app.additionally_active);

        // Stage into Bucket -> should loop back to ActionMenu with context intact
        app.execute_command("action.bucket");
        assert!(matches!(app.mode, AppMode::ActionMenu));
        assert_eq!(app.active_context.as_ref(), Some(&path));
        assert!(!app.additionally_active);
    }

    #[tokio::test]
    async fn test_missing_history_triggers_file_not_found_prompt() {
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let cfg = LoadedConfig {
            app: Default::default(),
            projects: Default::default(),
        };
        let mut app = App::new_in_memory(cfg, a_tx);
        let missing = PathBuf::from("/tmp/definitely_non_existent_mac_tui_fm_file_99999");
        app.open_history_entry(missing.clone());
        assert!(matches!(app.mode, AppMode::FileNotFoundPrompt { ref path } if path == &missing));
    }

    #[tokio::test]
    async fn test_home_h_key_opens_history() {
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let cfg = LoadedConfig {
            app: Default::default(),
            projects: Default::default(),
        };
        let mut app = App::new_in_memory(cfg, a_tx);
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert!(matches!(app.mode, AppMode::History(_)));
    }

    #[tokio::test]
    async fn test_move_creates_missing_dir_and_records_both_history() {
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let cfg = LoadedConfig {
            app: Default::default(),
            projects: Default::default(),
        };
        let mut app = App::new_in_memory(cfg, a_tx);

        let base = std::env::temp_dir().join(format!("mac-tui-fm-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let src = base.join("sample.txt");
        fs::write(&src, "hello").unwrap();

        let new_dir = base.join("nested/non-existent-dir");
        app.select_active_context(src.clone());
        app.execute_target_dir_transfer(new_dir.clone(), TargetDirIntent::Move);

        let dest = new_dir.join("sample.txt");
        assert!(dest.exists());
        assert!(!src.exists());

        let history = app.history.list();
        assert!(history.contains(&src));
        assert!(history.contains(&dest));

        let _ = fs::remove_dir_all(&base);
    }
}
