//! The macro table — GLSL 4.60 §3.3.
//!
//! Definitions are kept for the whole file, not just for as long as they are
//! live: an `#undef`ed macro stays in [`MacroTable::all`] with its definition
//! span so go-to-definition still answers for the lines that used it, and a
//! redefinition appends rather than overwrites. Only [`MacroTable::lookup`]
//! follows the live view.

use std::collections::HashMap;

use analyzer_core::spans::ByteSpan;

use super::PpToken;

/// The macros the preprocessor answers for without anyone defining them.
///
/// `__LINE__`, `__FILE__` and `__VERSION__` have no fixed body — their value
/// depends on where they are used — so they are marked [`MacroKind::Dynamic`]
/// and expanded by the expander itself. `GL_ES` is an ordinary object-like
/// macro, defined only when `#version … es` says so.
pub const DYNAMIC_MACROS: [&str; 3] = ["__LINE__", "__FILE__", "__VERSION__"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroKind {
    /// `#define NAME body`.
    Object,
    /// `#define NAME(a, b) body`.
    Function,
    /// A predefined macro whose value is computed at each use site.
    Dynamic,
}

/// One `#define`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDef {
    /// A number that is stable for the *name*, not for this definition —
    /// redefining a macro reuses the id. The expander's hide sets are keyed on
    /// it, which is what stops `#define X X` from looping.
    pub id: u32,
    pub name: String,
    pub kind: MacroKind,
    /// Parameter names, in order. Empty for object-like and dynamic macros;
    /// also empty for `#define F() …`, which [`MacroKind::Function`]
    /// distinguishes.
    pub params: Vec<String>,
    /// The replacement list. Every token's span points into the `#define`.
    pub body: Vec<PpToken>,
    /// The whole directive, `#` to end of line.
    pub span: ByteSpan,
    /// Just the name, for go-to-definition and rename.
    pub name_span: ByteSpan,
    /// Whether this came from the language or the host rather than the source.
    pub predefined: bool,
    /// Set when a later `#undef` retired this definition.
    pub undefined_at: Option<ByteSpan>,
}

impl MacroDef {
    /// Whether two definitions are "identical" in the sense §3.3 requires for a
    /// legal redefinition: same kind, same parameter names, same replacement
    /// list, same whitespace separation. Leading whitespace on the first body
    /// token does not count, matching glslang.
    pub fn is_identical_to(&self, other: &MacroDef) -> bool {
        if self.kind != other.kind || self.params != other.params {
            return false;
        }
        if self.body.len() != other.body.len() {
            return false;
        }
        self.body.iter().zip(&other.body).enumerate().all(|(i, (a, b))| {
            a.kind == b.kind
                && a.text == b.text
                && (i == 0 || a.leading_space == b.leading_space)
        })
    }
}

/// Every macro the file ever defined, plus the live view.
#[derive(Debug, Clone)]
pub struct MacroTable {
    /// Every definition, in source order, including retired ones.
    all: Vec<MacroDef>,
    /// Name → index into `all` for the definition that is live right now.
    live: HashMap<String, usize>,
    /// Name → id, so a redefinition keeps the id its name already had.
    ids: HashMap<String, u32>,
    next_id: u32,
    /// How many live macros start with each byte.
    ///
    /// The expander asks [`MacroTable::lookup`] about **every identifier in
    /// the file**, and a shader typically defines a handful of macros — so
    /// almost every one of those questions is answered "no" after hashing a
    /// string against a table that could not have held it. This is the cheap
    /// no: a live macro starting with `h` is the only thing that can make
    /// `helper` worth looking up. Measured at ~4 % of a full rebuild before it
    /// existed (RFC 012 P5-10).
    first_bytes: [u16; 256],
}

impl Default for MacroTable {
    fn default() -> MacroTable {
        MacroTable {
            all: Vec::new(),
            live: HashMap::new(),
            ids: HashMap::new(),
            next_id: 0,
            first_bytes: [0; 256],
        }
    }
}

impl MacroTable {
    pub fn new() -> Self {
        MacroTable::default()
    }

    /// Whether any live macro could be named `name` — first byte only, so a
    /// `false` is certain and a `true` still has to be looked up.
    #[inline]
    fn could_be_live(&self, name: &str) -> bool {
        match name.as_bytes().first() {
            Some(byte) => self.first_bytes[*byte as usize] > 0,
            None => false,
        }
    }

    /// The id reserved for `name`, minting one if this is its first sighting.
    pub fn id_for(&mut self, name: &str) -> u32 {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.ids.insert(name.to_string(), id);
        id
    }

    /// The definition in force for `name`, if any.
    pub fn lookup(&self, name: &str) -> Option<&MacroDef> {
        if !self.could_be_live(name) {
            return None;
        }
        self.live.get(name).map(|i| &self.all[*i])
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.could_be_live(name) && self.live.contains_key(name)
    }

    /// Every definition the file has ever held, retired ones included, in
    /// source order.
    pub fn all(&self) -> &[MacroDef] {
        &self.all
    }

    /// The definitions still in force at end of file, in source order.
    pub fn live(&self) -> impl Iterator<Item = &MacroDef> {
        self.all.iter().filter(|d| d.undefined_at.is_none())
    }

    /// Record a definition and make it the live one.
    pub fn define(&mut self, def: MacroDef) {
        let name = def.name.clone();
        let first = name.as_bytes().first().copied();
        self.all.push(def);
        // A redefinition replaces a live entry rather than adding one, so the
        // count only moves when the name was not already live.
        if self.live.insert(name, self.all.len() - 1).is_none() {
            if let Some(byte) = first {
                self.first_bytes[byte as usize] += 1;
            }
        }
    }

    /// Retire whatever `name` is bound to. Returns whether anything was bound.
    pub fn undefine(&mut self, name: &str, at: ByteSpan) -> bool {
        match self.live.remove(name) {
            Some(index) => {
                self.all[index].undefined_at = Some(at);
                if let Some(byte) = name.as_bytes().first() {
                    self.first_bytes[*byte as usize] -= 1;
                }
                true
            }
            None => false,
        }
    }
}

/// Whether `name` is one the language reserves, and how loudly to say so.
///
/// Mirrors glslang's `reservedPpErrorCheck`: `GL_`-prefixed names and `defined`
/// are hard errors, while names merely containing `__` are advisory — except
/// the three dynamic predefines, which are an error to touch anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reserved {
    No,
    /// Reserved, and redefining it is an error.
    Error,
    /// Reserved, and redefining it is undefined behaviour but not an error.
    Warning,
}

pub fn reserved(name: &str) -> Reserved {
    if name.starts_with("GL_") || name == "defined" || DYNAMIC_MACROS.contains(&name) {
        Reserved::Error
    } else if name.contains("__") {
        Reserved::Warning
    } else {
        Reserved::No
    }
}
