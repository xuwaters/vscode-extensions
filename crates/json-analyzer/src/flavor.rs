//! The four dialects this analyzer understands.
//!
//! The parser accepts the JSON5 superset for every flavor; the flavor
//! only decides which of the extensions get *diagnosed*. That keeps
//! recovery uniform — a stray comment in strict JSON still parses, still
//! folds, still formats; it just carries an error.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// RFC 8259. No comments, no trailing commas, double quotes only.
    Json,
    /// VSCode-style JSON with comments; trailing commas tolerated too,
    /// since the files people open as `jsonc` (tsconfig, settings)
    /// routinely carry them.
    Jsonc,
    /// Full JSON5: unquoted keys, single quotes, hex numbers,
    /// `Infinity`/`NaN`, line continuations, the lot.
    Json5,
    /// JSON Lines: one strict-JSON value per line.
    Jsonl,
}

impl Flavor {
    /// Maps a VSCode language id. Unknown ids get strict JSON, the
    /// least permissive reading.
    pub fn from_language_id(id: &str) -> Flavor {
        match id {
            "jsonc" => Flavor::Jsonc,
            "json5" => Flavor::Json5,
            "jsonl" | "ndjson" => Flavor::Jsonl,
            _ => Flavor::Json,
        }
    }

    pub fn allows_comments(self) -> bool {
        matches!(self, Flavor::Jsonc | Flavor::Json5)
    }

    pub fn allows_trailing_commas(self) -> bool {
        matches!(self, Flavor::Jsonc | Flavor::Json5)
    }

    /// Single quotes, unquoted keys, extended numbers, escape
    /// extensions — everything beyond "JSON plus comments".
    pub fn allows_json5_syntax(self) -> bool {
        matches!(self, Flavor::Json5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_ids_map_to_flavors() {
        assert_eq!(Flavor::from_language_id("json"), Flavor::Json);
        assert_eq!(Flavor::from_language_id("jsonc"), Flavor::Jsonc);
        assert_eq!(Flavor::from_language_id("json5"), Flavor::Json5);
        assert_eq!(Flavor::from_language_id("jsonl"), Flavor::Jsonl);
        assert_eq!(Flavor::from_language_id("ndjson"), Flavor::Jsonl);
        assert_eq!(Flavor::from_language_id("anything-else"), Flavor::Json);
    }
}
