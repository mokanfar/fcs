use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

    #[test]
    fn test_plain_char_token() {
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(key_event_to_token(key).as_deref(), Some("f"));
    }

    #[test]
    fn test_modifier_char_token() {
        let key = KeyEvent::new(KeyCode::Char('P'), KeyModifiers::CONTROL);
        assert_eq!(key_event_to_token(key).as_deref(), Some("ctrl+p"));
    }

    #[test]
    fn test_non_char_returns_none() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_event_to_token(key), None);
    }
}
