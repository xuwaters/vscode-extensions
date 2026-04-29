//! Tokenize a single fish-shell command line into a flat list of words.
//!
//! Handles enough of fish's quoting rules to faithfully extract the
//! contents of `complete` directives in `share/completions/*.fish`:
//!
//! - Bareword splitting on unescaped whitespace
//! - Single quotes `'…'` with `\\` and `\'` escapes only
//! - Double quotes `"…"` with `\\`, `\"`, `\$`, `\n`, `\t` escapes;
//!   `$var` is preserved as the literal text `$var` (we cannot resolve)
//! - Backslash line continuation `\<newline>`
//! - Comments after unquoted `#` are stripped
//!
//! What we do **not** support:
//!
//! - Command substitution `(...)` — we keep the literal text, including
//!   parens, as a single bareword. Predicates and `-a` argument lists
//!   that wrap a `(__fish_…)` call thus end up with the raw text and
//!   are filtered out one layer up by the parser.
//! - Brace expansion, glob expansion, variable expansion.
//! - The `&`, `|`, `;`, `<`, `>` operators (none appear inside a
//!   `complete` directive line).

/// Tokenize one logical line (already line-joined across `\<newline>`).
///
/// Returns the words verbatim. An unterminated quote yields `None`,
/// signalling the caller to drop the line.
pub fn tokenize(line: &str) -> Option<Vec<String>> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < bytes.len() {
        // Skip whitespace between tokens.
        while i < bytes.len() && is_blank(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // A `#` outside any quote starts a comment; drop the rest.
        if bytes[i] == b'#' {
            break;
        }

        let mut tok = String::new();
        let mut saw_anything = false;

        'word: while i < bytes.len() {
            let c = bytes[i];
            if is_blank(c) {
                break 'word;
            }
            saw_anything = true;
            match c {
                b'\'' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            let n = bytes[i + 1];
                            if n == b'\\' || n == b'\'' {
                                tok.push(n as char);
                                i += 2;
                                continue;
                            }
                        }
                        tok.push(bytes[i] as char);
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return None; // unterminated single quote
                    }
                    i += 1; // consume closing '
                }
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            let n = bytes[i + 1];
                            match n {
                                b'\\' | b'"' | b'$' => {
                                    tok.push(n as char);
                                    i += 2;
                                    continue;
                                }
                                b'n' => {
                                    tok.push('\n');
                                    i += 2;
                                    continue;
                                }
                                b't' => {
                                    tok.push('\t');
                                    i += 2;
                                    continue;
                                }
                                _ => {
                                    // Preserve the backslash + char verbatim.
                                    tok.push('\\');
                                    tok.push(n as char);
                                    i += 2;
                                    continue;
                                }
                            }
                        }
                        // Don't try to expand $var; keep the raw text.
                        tok.push(bytes[i] as char);
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return None; // unterminated double quote
                    }
                    i += 1; // consume closing "
                }
                b'\\' => {
                    if i + 1 < bytes.len() {
                        // Backslash escape outside quotes: emit the next
                        // character verbatim. (Line-continuation has
                        // already been handled by the caller.)
                        tok.push(bytes[i + 1] as char);
                        i += 2;
                    } else {
                        tok.push('\\');
                        i += 1;
                    }
                }
                b'(' => {
                    // Balanced command substitution — copy through with
                    // depth tracking so we don't mis-tokenise on inner
                    // whitespace. Anything we collect is treated as one
                    // bareword piece by the caller.
                    let mut depth = 1;
                    tok.push('(');
                    i += 1;
                    while i < bytes.len() && depth > 0 {
                        let c2 = bytes[i];
                        match c2 {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            b'\'' => {
                                tok.push(c2 as char);
                                i += 1;
                                while i < bytes.len() && bytes[i] != b'\'' {
                                    tok.push(bytes[i] as char);
                                    i += 1;
                                }
                                if i < bytes.len() {
                                    tok.push('\'');
                                    i += 1;
                                }
                                continue;
                            }
                            b'"' => {
                                tok.push(c2 as char);
                                i += 1;
                                while i < bytes.len() && bytes[i] != b'"' {
                                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                                        tok.push(bytes[i] as char);
                                        tok.push(bytes[i + 1] as char);
                                        i += 2;
                                        continue;
                                    }
                                    tok.push(bytes[i] as char);
                                    i += 1;
                                }
                                if i < bytes.len() {
                                    tok.push('"');
                                    i += 1;
                                }
                                continue;
                            }
                            _ => {}
                        }
                        tok.push(c2 as char);
                        i += 1;
                    }
                    if depth != 0 {
                        return None;
                    }
                }
                _ => {
                    tok.push(c as char);
                    i += 1;
                }
            }
        }

        if saw_anything {
            tokens.push(tok);
        }
    }

    Some(tokens)
}

fn is_blank(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

/// Pre-process a multi-line file into logical lines, joining backslash
/// line-continuations and stripping the trailing newline.
///
/// Each returned `String` is one logical line ready for [`tokenize`].
pub fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut iter = text.lines().peekable();
    while let Some(line) = iter.next() {
        // Trim a single trailing CR (CRLF support).
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(stripped) = trailing_continuation(line) {
            buf.push_str(stripped);
            buf.push(' ');
            // Keep going — the next physical line continues this one.
            if iter.peek().is_some() {
                continue;
            }
        } else {
            buf.push_str(line);
        }
        out.push(std::mem::take(&mut buf));
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// If `line` ends with `\` (and not `\\`), return the prefix without the
/// continuation marker.
fn trailing_continuation(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if !bytes.ends_with(b"\\") {
        return None;
    }
    // Count trailing backslashes; an odd count is a real continuation.
    let mut n = 0;
    for b in bytes.iter().rev() {
        if *b == b'\\' {
            n += 1;
        } else {
            break;
        }
    }
    if n % 2 == 1 {
        Some(&line[..line.len() - 1])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<String> {
        tokenize(s).unwrap()
    }

    #[test]
    fn bareword_split() {
        assert_eq!(t("complete -c curl"), vec!["complete", "-c", "curl"]);
        assert_eq!(t("  a   b\tc  "), vec!["a", "b", "c"]);
    }

    #[test]
    fn single_quotes_literal() {
        assert_eq!(t("'hello world'"), vec!["hello world"]);
        assert_eq!(t(r"'it\'s ok'"), vec!["it's ok"]);
        assert_eq!(t(r"'a\\b'"), vec![r"a\b"]);
    }

    #[test]
    fn double_quotes_with_escapes() {
        assert_eq!(t(r#""hi $there""#), vec!["hi $there"]);
        assert_eq!(t(r#""a\"b""#), vec![r#"a"b"#]);
        assert_eq!(t(r#""line\nbreak""#), vec!["line\nbreak"]);
    }

    #[test]
    fn comment_strips_rest() {
        assert_eq!(t("a b # c d"), vec!["a", "b"]);
        assert_eq!(t("'#'"), vec!["#"]);
    }

    #[test]
    fn unterminated_quote_returns_none() {
        assert!(tokenize("'oh no").is_none());
        assert!(tokenize(r#""oh no"#).is_none());
    }

    #[test]
    fn paren_substitution_kept_as_bareword() {
        assert_eq!(
            t("-a '(__fish_complete_directories)'"),
            vec!["-a", "(__fish_complete_directories)"]
        );
        // Unquoted paren substitution: also held together as one token.
        assert_eq!(t("-a (foo bar)"), vec!["-a", "(foo bar)"]);
    }

    #[test]
    fn line_continuation_join() {
        let text = "complete -c curl \\\n  -l verbose";
        let lines = logical_lines(text);
        assert_eq!(lines.len(), 1);
        assert_eq!(t(&lines[0]), vec!["complete", "-c", "curl", "-l", "verbose"]);
    }

    #[test]
    fn backslash_in_word_escapes() {
        assert_eq!(t(r"a\ b"), vec!["a b"]);
    }
}
