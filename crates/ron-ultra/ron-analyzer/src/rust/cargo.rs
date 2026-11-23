//! Just enough of `Cargo.toml` to find a crate's root file and the local
//! crates it depends on.
//!
//! A hand-rolled reader for the TOML subset manifests use: `[table]`
//! headers, `key = value` with dotted keys, strings, inline tables, and
//! arrays (skipped, even across lines). Anything else on a line is ignored.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// `[package] name`, with `-` turned into `_` as Rust sees it.
    pub package: Option<String>,
    /// `[lib] name`, normalized the same way.
    pub lib_name: Option<String>,
    /// `[lib] path`, relative to the manifest.
    pub lib_path: Option<String>,
    /// `[workspace]` is present.
    pub is_workspace: bool,
    /// From `[dependencies]` and `[dev-dependencies]`.
    pub dependencies: Vec<Dependency>,
    /// From `[workspace.dependencies]`.
    pub workspace_dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependency {
    /// The name code uses, `-` normalized to `_`.
    pub name: String,
    /// `path = "..."`, relative to the manifest that says it.
    pub path: Option<String>,
    /// `workspace = true`: look it up in the workspace root.
    pub workspace: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Bool(bool),
    Table(Vec<(Vec<String>, Value)>),
    Other,
}

pub fn parse_manifest(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut table: Vec<String> = Vec::new();
    let mut r = Reader { chars: text.chars().collect(), pos: 0 };
    while r.pos < r.chars.len() {
        r.skip_ws_and_newlines();
        let Some(c) = r.peek() else { break };
        if c == '#' {
            r.skip_line();
        } else if c == '[' {
            let array_table = r.chars.get(r.pos + 1) == Some(&'[');
            r.pos += if array_table { 2 } else { 1 };
            table = r.key();
            r.skip_line();
            if !array_table && table == ["workspace"] {
                m.is_workspace = true;
            }
            if array_table {
                // `[[bin]]` and friends: nothing we read lives there.
                table = vec!["<array>".into()];
            }
        } else {
            let key = r.key();
            r.skip_spaces();
            if r.peek() != Some('=') || key.is_empty() {
                r.skip_line();
                continue;
            }
            r.pos += 1;
            let value = r.value();
            r.skip_line();
            let mut full = table.clone();
            full.extend(key);
            m.apply(&full, value);
        }
    }
    m
}

fn normalize(name: &str) -> String {
    name.replace('-', "_")
}

impl Manifest {
    fn apply(&mut self, key: &[String], value: Value) {
        let k: Vec<&str> = key.iter().map(String::as_str).collect();
        match (k.as_slice(), &value) {
            (["package", "name"], Value::Str(s)) => self.package = Some(normalize(s)),
            (["lib", "name"], Value::Str(s)) => self.lib_name = Some(normalize(s)),
            (["lib", "path"], Value::Str(s)) => self.lib_path = Some(s.clone()),
            (["dependencies" | "dev-dependencies", name, rest @ ..], _) => {
                dep_entry(&mut self.dependencies, name, rest, value);
            }
            (["workspace", "dependencies", name, rest @ ..], _) => {
                dep_entry(&mut self.workspace_dependencies, name, rest, value);
            }
            _ => {}
        }
    }
}

/// Merge one `name = ...` / `name.key = ...` line into the dependency list.
fn dep_entry(list: &mut Vec<Dependency>, name: &str, rest: &[&str], value: Value) {
    let pos = list.iter().position(|d| d.name == normalize(name)).unwrap_or_else(|| {
        list.push(Dependency { name: normalize(name), ..Dependency::default() });
        list.len() - 1
    });
    let mut pairs: Vec<(Vec<String>, Value)> = Vec::new();
    match (rest, value) {
        ([], Value::Table(entries)) => pairs = entries,
        ([], _) => {}
        (keys, v) => pairs.push((keys.iter().map(|s| s.to_string()).collect(), v)),
    }
    for (k, v) in pairs {
        let dep = &mut list[pos];
        match (k.first().map(String::as_str), v) {
            (Some("path"), Value::Str(s)) => dep.path = Some(s),
            (Some("workspace"), Value::Bool(b)) => dep.workspace = b,
            // `package = "real-name"` renames the dependency, but the key —
            // the name code uses — stays what it was.
            _ => {}
        }
    }
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn skip_ws_and_newlines(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == '\n' {
                break;
            }
        }
    }

    /// A dotted key: `a.b."c-d"`, up to `=` or `]`.
    fn key(&mut self) -> Vec<String> {
        let mut parts = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('"' | '\'') => parts.push(self.string()),
                Some(c) if c.is_alphanumeric() || c == '_' || c == '-' => {
                    let start = self.pos;
                    while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                        self.pos += 1;
                    }
                    parts.push(self.chars[start..self.pos].iter().collect());
                }
                _ => break,
            }
            self.skip_spaces();
            if self.peek() == Some('.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        parts
    }

    fn string(&mut self) -> String {
        let quote = self.peek().unwrap_or('"');
        let triple = self.chars.get(self.pos..self.pos + 3).is_some_and(|s| s.iter().all(|&c| c == quote));
        if triple {
            self.pos += 3;
            let start = self.pos;
            while self.pos < self.chars.len()
                && !self.chars.get(self.pos..self.pos + 3).is_some_and(|s| s.iter().all(|&c| c == quote))
            {
                self.pos += 1;
            }
            let s: String = self.chars[start..self.pos.min(self.chars.len())].iter().collect();
            self.pos = (self.pos + 3).min(self.chars.len());
            return s;
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '\\' if quote == '"' => {
                    if let Some(e) = self.peek() {
                        self.pos += 1;
                        out.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                }
                '\n' => break,
                c if c == quote => break,
                c => out.push(c),
            }
        }
        out
    }

    fn value(&mut self) -> Value {
        self.skip_spaces();
        match self.peek() {
            Some('"' | '\'') => Value::Str(self.string()),
            Some('{') => {
                self.pos += 1;
                let mut entries = Vec::new();
                loop {
                    self.skip_ws_and_newlines();
                    match self.peek() {
                        None => break,
                        Some('}') => {
                            self.pos += 1;
                            break;
                        }
                        Some(',') => self.pos += 1,
                        _ => {
                            let key = self.key();
                            self.skip_spaces();
                            if key.is_empty() || self.peek() != Some('=') {
                                // Malformed: bail out of the table.
                                while self.peek().is_some_and(|c| c != '}' && c != '\n') {
                                    self.pos += 1;
                                }
                                continue;
                            }
                            self.pos += 1;
                            entries.push((key, self.value()));
                        }
                    }
                }
                Value::Table(entries)
            }
            Some('[') => {
                let mut depth = 0;
                while let Some(c) = self.peek() {
                    match c {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.pos += 1;
                                break;
                            }
                        }
                        '"' | '\'' => {
                            self.string();
                            continue;
                        }
                        '#' => {
                            self.skip_line();
                            continue;
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                Value::Other
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(|c| !matches!(c, ',' | '}' | '\n' | '#')) {
                    self.pos += 1;
                }
                let raw: String = self.chars[start..self.pos].iter().collect();
                match raw.trim() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => Value::Other,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn package_lib_and_dependencies() {
        let m = parse_manifest(
            r#"
# comment
[package]
name = "my-game"
version = "0.1.0"
authors = [
  "someone",  # with a comment
]
description = """
multi [line]
"""

[lib]
path = "src/game.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
shared-types = { path = "../shared" }
common.workspace = true

[dev-dependencies.tools]
path = "tools"

[[bin]]
name = "x"
path = "src/x.rs"
"#,
        );
        assert_eq!(m.package.as_deref(), Some("my_game"));
        assert_eq!(m.lib_path.as_deref(), Some("src/game.rs"));
        assert!(!m.is_workspace);
        let deps: Vec<(String, Option<String>, bool)> =
            m.dependencies.iter().map(|d| (d.name.clone(), d.path.clone(), d.workspace)).collect();
        assert_eq!(
            deps,
            vec![
                ("serde".into(), None, false),
                ("shared_types".into(), Some("../shared".into()), false),
                ("common".into(), None, true),
                ("tools".into(), Some("tools".into()), false),
            ]
        );
    }

    #[test]
    fn workspace_root() {
        let m = parse_manifest("[workspace]\nmembers = [\"a\"]\n\n[workspace.dependencies]\ncommon = { path = \"crates/common\" }\n");
        assert!(m.is_workspace);
        assert_eq!(m.package, None);
        assert_eq!(m.workspace_dependencies[0].path.as_deref(), Some("crates/common"));
    }
}
