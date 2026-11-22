use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_KEYBINDINGS_JSON: &str = include_str!("../config/keybindings.json");
const DEFAULT_PROJECTS_JSON: &str = include_str!("../config/projects.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub show_hidden_files: bool,
    #[serde(default = "default_editor")]
    pub editor: String,
    #[serde(default = "default_search_roots")]
    pub search_roots: Vec<String>,
}

fn default_editor() -> String {
    "subl".to_string()
}

fn default_search_roots() -> Vec<String> {
    vec![
        "~/Desktop".to_string(),
        "~/Downloads".to_string(),
        "~/Documents".to_string(),
    ]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_hidden_files: false,
            editor: default_editor(),
            search_roots: default_search_roots(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybindings {
    #[serde(default = "default_home_screen_bindings")]
    pub home_screen: HashMap<String, String>,
    #[serde(default = "default_action_menu_bindings")]
    pub action_menu: HashMap<String, String>,
}

fn default_home_screen_bindings() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("f".into(), "search.find_file".into());
    map.insert("d".into(), "search.find_directory".into());
    map.insert("c".into(), "search.find_content".into());
    map.insert("r".into(), "search.recents".into());
    map.insert("h".into(), "search.history".into());
    map.insert("p".into(), "search.projects".into());
    map.insert("b".into(), "bucket.open".into());
    map
}

fn default_action_menu_bindings() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("m".into(), "action.move".into());
    map.insert("c".into(), "submenu.copy".into());
    map.insert("x".into(), "action.cut".into());
    map.insert("e".into(), "action.edit".into());
    map.insert("b".into(), "action.bucket".into());
    map.insert("o".into(), "submenu.open".into());
    map.insert("p".into(), "action.paste".into());
    map.insert("h".into(), "nav.home".into());
    map
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            home_screen: default_home_screen_bindings(),
            action_menu: default_action_menu_bindings(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub keybindings: Keybindings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectsConfig {
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub app: AppConfig,
    pub projects: ProjectsConfig,
}

impl LoadedConfig {
    pub fn load_or_init() -> Result<Self> {
        let dir = config_dir();
        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
        }

        let kb_path = dir.join("keybindings.json");
        if !kb_path.exists() {
            let _ = fs::write(&kb_path, DEFAULT_KEYBINDINGS_JSON);
        }

        let projects_path = dir.join("projects.json");
        if !projects_path.exists() {
            let _ = fs::write(&projects_path, DEFAULT_PROJECTS_JSON);
        }

        let mut app = fs::read_to_string(&kb_path)
            .ok()
            .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
            .unwrap_or_default();

        // Ensure existing configs from older versions still get `h` -> `search.history`
        let has_history_cmd = app
            .keybindings
            .home_screen
            .values()
            .any(|v| v == "search.history");
        if !has_history_cmd && !app.keybindings.home_screen.contains_key("h") {
            app.keybindings
                .home_screen
                .insert("h".into(), "search.history".into());
        }

        let projects = fs::read_to_string(&projects_path)
            .ok()
            .and_then(|s| serde_json::from_str::<ProjectsConfig>(&s).ok())
            .unwrap_or_else(|| {
                serde_json::from_str::<ProjectsConfig>(DEFAULT_PROJECTS_JSON).unwrap_or_default()
            });

        Ok(Self { app, projects })
    }

    pub fn expanded_search_roots(&self) -> Vec<PathBuf> {
        self.app
            .settings
            .search_roots
            .iter()
            .map(|p| expand_tilde(p))
            .collect()
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("mac-tui-fm")
}

pub fn history_path() -> PathBuf {
    config_dir().join("history.json")
}

pub fn bucket_path() -> PathBuf {
    config_dir().join("bucket.json")
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Resolves a user-typed directory path against configured search roots or home directory.
/// Example: `desktop/non-existent-dir` matches `~/Desktop` -> `/Users/<user>/Desktop/non-existent-dir`.
pub fn resolve_target_dir(input: &str, search_roots: &[PathBuf]) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.starts_with('/') || trimmed.starts_with('~') {
        return expand_tilde(trimmed);
    }

    // Check if the first path component matches any search_root basename case-insensitively
    let mut parts = trimmed.splitn(2, '/');
    let first = parts.next().unwrap_or("");
    let rest = parts.next();

    for root in search_roots {
        if let Some(name) = root.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case(first) {
                return match rest {
                    Some(r) if !r.is_empty() => root.join(r),
                    _ => root.clone(),
                };
            }
        }
    }

    // Fallback: relative to first search root, or home dir
    if let Some(home) = dirs::home_dir() {
        home.join(trimmed)
    } else {
        Path::new(trimmed).to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_parses() {
        let app: AppConfig = serde_json::from_str(DEFAULT_KEYBINDINGS_JSON).unwrap();
        assert_eq!(app.settings.editor, "subl");
        assert_eq!(app.settings.search_roots.len(), 3);
        assert_eq!(
            app.keybindings.home_screen.get("f").map(String::as_str),
            Some("search.find_file")
        );
        assert_eq!(
            app.keybindings.home_screen.get("h").map(String::as_str),
            Some("search.history")
        );
        assert_eq!(
            app.keybindings.action_menu.get("p").map(String::as_str),
            Some("action.paste")
        );

        let projects: ProjectsConfig = serde_json::from_str(DEFAULT_PROJECTS_JSON).unwrap();
        assert_eq!(projects.projects.len(), 2);
    }

    #[test]
    fn test_resolve_target_dir_matches_root_case_insensitive() {
        let roots = vec![
            PathBuf::from("/Users/dev/Desktop"),
            PathBuf::from("/Users/dev/Documents"),
        ];
        let resolved = resolve_target_dir("desktop/non-existent-dir", &roots);
        assert_eq!(
            resolved,
            PathBuf::from("/Users/dev/Desktop/non-existent-dir")
        );
    }
}
