use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordResolution {
    /// Immediately resolved to a command (no longer chord shares this prefix).
    Exact(String),
    /// Waiting for potential subsequent key(s) within `chord_timeout_ms`.
    Pending {
        buffer: String,
        fallback: Option<String>,
    },
    /// Buffer did not match any binding.
    NoMatch,
}

/// Non-blocking chord parser implementing the 1-second timeout disambiguation rule.
pub struct ChordParser {
    buffer: String,
    pending_fallback: Option<String>,
    timeout_ms: u64,
    timer_handle: Option<JoinHandle<()>>,
    timeout_tx: mpsc::UnboundedSender<u64>,
    generation: u64,
}

impl ChordParser {
    pub fn new(timeout_ms: u64, timeout_tx: mpsc::UnboundedSender<u64>) -> Self {
        Self {
            buffer: String::new(),
            pending_fallback: None,
            timeout_ms,
            timer_handle: None,
            timeout_tx,
            generation: 0,
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.pending_fallback = None;
        self.cancel_timer();
    }

    fn cancel_timer(&mut self) {
        if let Some(handle) = self.timer_handle.take() {
            handle.abort();
        }
    }

    fn arm_timer(&mut self) {
        self.cancel_timer();
        self.generation = self.generation.wrapping_add(1);
        let gen = self.generation;
        let tx = self.timeout_tx.clone();
        let ms = self.timeout_ms;
        self.timer_handle = Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            let _ = tx.send(gen);
        }));
    }

    /// Processes an incoming key token against the active mode's chord map.
    /// Returns a list of 0..=2 resolved commands (e.g., if a previous ambiguous prefix
    /// had to flush before processing an unrelated new key).
    pub fn feed(
        &mut self,
        token: &str,
        bindings: &HashMap<String, String>,
    ) -> Vec<ChordResolution> {
        let candidate = format!("{}{}", self.buffer, token);
        let exact = bindings.get(&candidate).cloned();
        let has_longer_prefix = bindings
            .keys()
            .any(|k| k.len() > candidate.len() && k.starts_with(&candidate));

        match (exact, has_longer_prefix) {
            (Some(cmd), false) => {
                // Disambiguated exact match!
                self.clear();
                vec![ChordResolution::Exact(cmd)]
            }
            (exact_opt, true) => {
                // Prefix of a longer chord (and possibly also a standalone exact match like 'o' vs 'od')
                self.buffer = candidate.clone();
                self.pending_fallback = exact_opt.clone();
                self.arm_timer();
                vec![ChordResolution::Pending {
                    buffer: candidate,
                    fallback: exact_opt,
                }]
            }
            (None, false) => {
                // Combined buffer doesn't match anything.
                // If we had a pending exact fallback (e.g., 'o' followed by invalid key 'z'),
                // flush 'o' first, then re-evaluate token fresh.
                if !self.buffer.is_empty() {
                    let prev_fallback = self.pending_fallback.take();
                    self.clear();
                    let mut results = Vec::new();
                    if let Some(fb) = prev_fallback {
                        results.push(ChordResolution::Exact(fb));
                    }
                    // Re-evaluate the new token from an empty buffer
                    let sub = self.feed(token, bindings);
                    results.extend(sub);
                    results
                } else {
                    self.clear();
                    vec![ChordResolution::NoMatch]
                }
            }
        }
    }

    /// Called when the Tokio timeout channel fires with a generation ID.
    pub fn on_timeout(&mut self, gen: u64) -> Option<String> {
        if gen != self.generation {
            return None;
        }
        self.timer_handle = None;
        let cmd = self.pending_fallback.take();
        self.buffer.clear();
        cmd
    }
}

/// Converts a Crossterm `KeyEvent` into a normalized keybinding token string.
pub fn key_event_to_token(key: KeyEvent) -> Option<String> {
    let mut prefix = String::new();
    if key.modifiers.contains(KeyModifiers::SUPER) {
        prefix.push_str("cmd+");
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        prefix.push_str("ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        prefix.push_str("alt+");
    }

    match key.code {
        KeyCode::Char(c) => {
            if prefix.is_empty() {
                Some(c.to_string())
            } else {
                Some(format!("{}{}", prefix, c.to_ascii_lowercase()))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bindings() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("f".into(), "search.find_file".into());
        m.insert("o".into(), "submenu.open".into());
        m.insert("od".into(), "action.open_dir".into());
        m.insert("of".into(), "action.open_file".into());
        m.insert("pa".into(), "action.paste".into());
        m
    }

    #[tokio::test]
    async fn test_immediate_exact_match() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut parser = ChordParser::new(1000, tx);
        let bindings = sample_bindings();
        let res = parser.feed("f", &bindings);
        assert_eq!(res, vec![ChordResolution::Exact("search.find_file".into())]);
        assert!(parser.buffer().is_empty());
    }

    #[tokio::test]
    async fn test_disambiguated_chord_before_timeout() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut parser = ChordParser::new(1000, tx);
        let bindings = sample_bindings();

        let res1 = parser.feed("o", &bindings);
        assert_eq!(
            res1,
            vec![ChordResolution::Pending {
                buffer: "o".into(),
                fallback: Some("submenu.open".into()),
            }]
        );

        let res2 = parser.feed("d", &bindings);
        assert_eq!(res2, vec![ChordResolution::Exact("action.open_dir".into())]);
        assert!(parser.buffer().is_empty());
    }

    #[tokio::test]
    async fn test_timeout_executes_standalone_command() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut parser = ChordParser::new(30, tx);
        let bindings = sample_bindings();

        let res1 = parser.feed("o", &bindings);
        assert!(matches!(res1[0], ChordResolution::Pending { .. }));

        let gen = rx.recv().await.expect("timeout should fire");
        let resolved = parser.on_timeout(gen);
        assert_eq!(resolved, Some("submenu.open".into()));
        assert!(parser.buffer().is_empty());
    }

    #[tokio::test]
    async fn test_prefix_only_chord_pa() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut parser = ChordParser::new(30, tx);
        let bindings = sample_bindings();

        let res1 = parser.feed("p", &bindings);
        assert_eq!(
            res1,
            vec![ChordResolution::Pending {
                buffer: "p".into(),
                fallback: None,
            }]
        );

        let res2 = parser.feed("a", &bindings);
        assert_eq!(res2, vec![ChordResolution::Exact("action.paste".into())]);

        // Also verify timeout on incomplete 'p' resolves to None
        let _ = parser.feed("p", &bindings);
        let gen = rx.recv().await.expect("timeout should fire");
        assert_eq!(parser.on_timeout(gen), None);
    }
}
