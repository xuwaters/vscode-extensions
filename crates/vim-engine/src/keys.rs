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

    /// How a key reads in the status bar's pending keys — the notation it is
    /// written in, so a buffered `<space>` is something you can see.
    pub fn label(self) -> String {
        match self {
            Key::Char(' ') => "<space>".to_string(),
            Key::Char(c) => c.to_string(),
            Key::Esc => "<esc>".to_string(),
            Key::Enter => "<cr>".to_string(),
            Key::Backspace => "<bs>".to_string(),
            Key::Ctrl(c) => format!("<c-{c}>"),
        }
    }
}

/// Parse a key sequence the way a Vim mapping is written: `<space><space>`,
/// `\\`, `,w`. Unknown `<…>` names are dropped rather than taken apart, and a
/// `<` with no `>` after it is the character itself.
pub fn parse_sequence(s: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '<' {
            out.push(Key::Char(ch));
            continue;
        }
        let mut name = String::from("<");
        for c in chars.by_ref() {
            name.push(c);
            if c == '>' {
                break;
            }
        }
        if name.ends_with('>') && name.len() > 2 {
            out.extend(Key::parse(&name));
        } else {
            out.extend(name.chars().map(Key::Char));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sequences() {
        assert_eq!(
            parse_sequence("<space><space>"),
            [Key::Char(' '), Key::Char(' ')]
        );
        assert_eq!(parse_sequence(",,"), [Key::Char(','), Key::Char(',')]);
        assert_eq!(parse_sequence("<c-s>k"), [Key::Ctrl('s'), Key::Char('k')]);
        assert_eq!(parse_sequence("<"), [Key::Char('<')]);
        assert_eq!(parse_sequence("<nope>x"), [Key::Char('x')]);
        assert!(parse_sequence("").is_empty());
    }

    #[test]
    fn labels_read_as_written() {
        assert_eq!(Key::Char(' ').label(), "<space>");
        assert_eq!(Key::Char('w').label(), "w");
        assert_eq!(Key::Ctrl('r').label(), "<c-r>");
    }

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
