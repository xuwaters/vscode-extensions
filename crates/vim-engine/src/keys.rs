//! Key normalization. The host sends printable keys as bare characters and
//! special keys in angle-bracket notation: `<esc>`, `<cr>`, `<bs>`, `<c-r>`.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Ctrl(char),
}

impl Key {
    /// Parse one key. Multi-char strings that are not angle-bracket specials
    /// yield their first char (the host splits `type` payloads per char).
    pub fn parse(s: &str) -> Option<Key> {
        if s.len() > 1 && s.starts_with('<') && s.ends_with('>') {
            let name = s[1..s.len() - 1].to_ascii_lowercase();
            return match name.as_str() {
                "esc" => Some(Key::Esc),
                "cr" | "enter" | "return" => Some(Key::Enter),
                "bs" => Some(Key::Backspace),
                "space" => Some(Key::Char(' ')),
                "lt" => Some(Key::Char('<')),
                _ => name
                    .strip_prefix("c-")
                    .and_then(|c| c.chars().next())
                    .map(Key::Ctrl),
            };
        }
        s.chars().next().map(Key::Char)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specials_and_chars() {
        assert_eq!(Key::parse("a"), Some(Key::Char('a')));
        assert_eq!(Key::parse("<"), Some(Key::Char('<')));
        assert_eq!(Key::parse("<Esc>"), Some(Key::Esc));
        assert_eq!(Key::parse("<c-r>"), Some(Key::Ctrl('r')));
        assert_eq!(Key::parse("<C-U>"), Some(Key::Ctrl('u')));
        assert_eq!(Key::parse("<lt>"), Some(Key::Char('<')));
        assert_eq!(Key::parse(""), None);
    }
}
