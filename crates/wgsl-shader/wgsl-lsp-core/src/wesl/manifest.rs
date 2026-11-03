//! `wesl.toml`, read for the two things module resolution needs: where
//! `package::` starts, and which directory each path dependency lives in.
//!
//! ```toml
//! edition = "2026_pre"
//! root = "./shaders"            # `package::` — "./shaders" when absent
//!
//! [dependencies]
//! mylib = { path = "../mylib" } # a folder holding its own wesl.toml
//! bevy = {}                     # from a package manager: not ours to find
//! ```
//!
//! Not a TOML parser. The manifest is a handful of flat keys and one table of
//! dependencies, and a real parser is a dependency the WASM bundle pays for on
//! every load. Lines this does not understand are skipped, which costs at
//! worst a dependency that does not resolve — never an error.

/// The default `root`, which is what `wesl-rs` assumes too.
pub const DEFAULT_ROOT: &str = "./shaders";

/// What a `wesl.toml` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The directory holding the manifest, as a URI with no trailing `/`.
    pub dir: String,
    /// `root`, relative to [`Manifest::dir`].
    pub root: String,
    /// `name → path`, for the dependencies that give one. Package-manager
    /// dependencies have no path and are left out.
    pub dependencies: Vec<(String, String)>,
}

impl Manifest {
    /// Read a manifest's text. `dir` is the URI of the folder it sits in.
    pub fn parse(dir: &str, text: &str) -> Manifest {
        let mut manifest = Manifest {
            dir: dir.trim_end_matches('/').to_string(),
            root: DEFAULT_ROOT.to_string(),
            dependencies: Vec::new(),
        };
        // The table the current line belongs to: `""` at the top,
        // `dependencies`, or `dependencies.<name>`.
        let mut table = String::new();

        for line in text.lines() {
            let line = strip_comment(line).trim();
            if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                table = header.trim().to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = unquote(key.trim());
            let value = value.trim();

            match table.as_str() {
                "" if key == "root" => {
                    if let Some(root) = string(value) {
                        manifest.root = root;
                    }
                }
                "dependencies" => {
                    if let Some(path) = inline_path(value) {
                        manifest.dependencies.push((key.to_string(), path));
                    }
                }
                _ => {
                    if let Some(name) = table.strip_prefix("dependencies.") {
                        if key == "path" {
                            if let Some(path) = string(value) {
                                manifest.dependencies.push((unquote(name).to_string(), path));
                            }
                        }
                    }
                }
            }
        }
        manifest
    }
}

/// Everything before a `#` that is not inside a string.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '#') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// A bare or quoted key.
fn unquote(key: &str) -> &str {
    key.trim_matches(|c| c == '"' || c == '\'')
}

/// A basic or literal string value. Escapes are left as written: a path that
/// needs them is not one a shader package has.
fn string(value: &str) -> Option<String> {
    let quote = value.chars().next().filter(|&c| c == '"' || c == '\'')?;
    let rest = &value[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// The `path` of an inline table: `{ path = "../mylib" }`.
fn inline_path(value: &str) -> Option<String> {
    let body = value.strip_prefix('{')?.trim_end().strip_suffix('}')?;
    body.split(',').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (unquote(key.trim()) == "path").then(|| string(value.trim())).flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_defaults_to_shaders() {
        let manifest = Manifest::parse("file:///p", "edition = \"2026_pre\"\n");
        assert_eq!(manifest.root, DEFAULT_ROOT);
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn root_and_path_dependencies_are_read_in_both_table_forms() {
        let manifest = Manifest::parse(
            "file:///p/",
            "edition = \"2026_pre\"\nroot = 'src/shaders' # where package:: starts\n\n\
             [dependencies]\nmylib = { path = \"../mylib\" }\nbevy = {}\n\
             \"quoted\" = { package = \"q\", path = \"vendor/q\" }\n\n\
             [dependencies.other]\npath = \"../other\"\n",
        );
        assert_eq!(manifest.dir, "file:///p");
        assert_eq!(manifest.root, "src/shaders");
        assert_eq!(
            manifest.dependencies,
            [
                ("mylib".to_string(), "../mylib".to_string()),
                ("quoted".to_string(), "vendor/q".to_string()),
                ("other".to_string(), "../other".to_string()),
            ]
        );
    }

    /// A `#` inside a string is part of the string.
    #[test]
    fn a_hash_in_a_string_is_not_a_comment() {
        let manifest = Manifest::parse("file:///p", "root = \"./sh#aders\"\n");
        assert_eq!(manifest.root, "./sh#aders");
    }
}
