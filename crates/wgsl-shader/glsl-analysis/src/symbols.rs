//! Symbols, scopes and what a name resolves to (P4-01).
//!
//! A [`Symbol`] is one declared name with its type and the qualifiers that
//! decide whether it can be written to. Symbols live in one flat arena, and
//! [`Scopes`] is a stack of name → id maps over it, so shadowing is "the
//! innermost frame that has the name" and needs no scope tree.
//!
//! Functions are the one name that can mean several things at once, so a frame
//! maps a name to a *list*: `float f(float)` and `vec2 f(vec2)` are two symbols
//! under one name and overload resolution picks between them. Every other kind
//! of redeclaration in one scope is [`crate::SemanticCode::Redeclaration`].

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use analyzer_core::spans::ByteSpan;

use crate::types::{StructId, Type};

/// The hasher the scope frames use, in place of the standard library's.
///
/// `HashMap`'s default is SipHash-1-3, chosen to survive an adversary who
/// picks the keys. A scope frame's keys are the identifiers of the file being
/// edited, the map lives for one analysis of one document, and nothing about
/// it is reachable from a network — so the resistance buys nothing and the
/// hashing showed up as ~4 % of a full rebuild (RFC 012 P5-10).
///
/// This is the FxHash rustc uses on its own symbol tables: one multiply and
/// one rotate per machine word. It is not a good hash for arbitrary data and
/// makes no claim to be; it is a good hash for short identifiers.
#[derive(Default)]
pub struct NameHasher(u64);

impl NameHasher {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    #[inline]
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(NameHasher::SEED);
    }
}

impl Hasher for NameHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while let Some((word, tail)) = rest.split_at_checked(8) {
            self.mix(u64::from_le_bytes(word.try_into().unwrap_or([0; 8])));
            rest = tail;
        }
        // The tail, as one more word. Its length goes in too, so that "ab" and
        // "ab\0" cannot collide by construction.
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.mix(u64::from_le_bytes(tail));
        self.mix(bytes.len() as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A name → ids map hashed by [`NameHasher`].
type NameMap = HashMap<String, Vec<SymbolId>, BuildHasherDefault<NameHasher>>;

/// A symbol's index in [`SymbolTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

impl SymbolId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a declaration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Struct,
    /// A file-scope variable: `uniform`, `in`, `out`, `buffer`, a global.
    Global,
    Parameter,
    Local,
    /// The interface name of a `uniform Camera { … }` — a type-like name the
    /// API binds against, not a value.
    Block,
    /// A member of a struct or of a named interface block. Members are reached
    /// through their owner, never through a scope, so these are recorded for
    /// go-to-definition but never entered into a frame.
    Field,
}

/// The storage and memory qualifiers that decide what may be done to a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Qualifiers {
    pub is_const: bool,
    pub is_uniform: bool,
    pub is_buffer: bool,
    /// `in`, `attribute`, or a function parameter with neither `out` nor
    /// `inout`.
    pub is_in: bool,
    pub is_out: bool,
    pub is_shared: bool,
    /// `varying`, which is `in` or `out` depending on the stage — and so is
    /// never treated as read-only.
    pub is_varying: bool,
}

impl Qualifiers {
    /// Whether assigning to this name is an error.
    ///
    /// Deliberately narrow. A `const` and a `uniform` are read-only in every
    /// dialect; a shader input is read-only too, but `in` on a *parameter*
    /// means "a copy the function may freely modify", so only the declaration
    /// site's `in` counts, and that is [`Qualifiers::is_in`] on a global.
    pub fn read_only(&self, kind: SymbolKind) -> Option<&'static str> {
        if self.is_const {
            return Some("const");
        }
        if self.is_uniform {
            return Some("uniform");
        }
        if self.is_in && matches!(kind, SymbolKind::Global) && !self.is_varying {
            return Some("shader input");
        }
        None
    }
}

/// One declared name.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub ty: Type,
    pub qualifiers: Qualifiers,
    /// The identifier alone.
    pub name_span: ByteSpan,
    /// The whole declaration this name came from.
    pub full_span: ByteSpan,
    /// A function's signature. `None` for everything else.
    pub signature: Option<Signature>,
    /// The struct or block this name *is*, for `SymbolKind::Struct` and
    /// `SymbolKind::Block`.
    pub struct_id: Option<StructId>,
    /// Whether the file only prototyped this function and never defined it.
    pub is_prototype: bool,
    /// The integer a `const` initialiser folded to, when it folded to one.
    /// This is what makes `const int N = 4; float samples[N];` a sized array
    /// rather than a mystery. See [`crate::consteval`].
    pub const_value: Option<i64>,
}

/// A user function's signature, as overload resolution needs it.
#[derive(Debug, Clone)]
pub struct Signature {
    pub ret: Type,
    pub params: Vec<Parameter>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    /// Whether the callee writes it — `out` or `inout`, which makes the
    /// argument have to be an lvalue.
    pub writes: bool,
}

impl Signature {
    /// The signature as GLSL writes it, for a diagnostic or a hover.
    pub fn display(&self, name: &str, structs: &crate::types::StructTable) -> String {
        let mut out = String::with_capacity(32 + self.params.len() * 12);
        out.push_str(&self.ret.name(structs));
        out.push(' ');
        out.push_str(name);
        out.push('(');
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if param.writes {
                out.push_str("out ");
            }
            out.push_str(&param.ty.name(structs));
        }
        out.push(')');
        out
    }
}

/// Every symbol a file declares.
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    pub fn push(&mut self, symbol: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(symbol);
        id
    }

    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.index())
    }

    pub fn get_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(id.index())
    }

    pub fn iter(&self) -> impl Iterator<Item = (SymbolId, &Symbol)> {
        self.symbols.iter().enumerate().map(|(i, s)| (SymbolId(i as u32), s))
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// One lexical scope: a file, a function body, a block, a `for` header.
#[derive(Debug, Clone, Default)]
struct Frame {
    names: NameMap,
}

/// The scope stack.
#[derive(Debug, Clone)]
pub struct Scopes {
    frames: Vec<Frame>,
}

impl Default for Scopes {
    fn default() -> Scopes {
        Scopes { frames: vec![Frame::default()] }
    }
}

impl Scopes {
    pub fn push(&mut self) {
        self.frames.push(Frame::default());
    }

    pub fn pop(&mut self) {
        // The file scope is never popped: an unbalanced walk must not leave the
        // table without one.
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Add a name to the innermost scope. Returns what it collides with, if it
    /// collides with anything — a function name never collides with another
    /// function, because that is what overloading is.
    pub fn declare(&mut self, name: &str, id: SymbolId, overloadable: bool) -> Option<SymbolId> {
        let frame = self.frames.last_mut().expect("the file scope is never popped");
        let entry = frame.names.entry(name.to_string()).or_default();
        let clash = if overloadable { None } else { entry.first().copied() };
        entry.push(id);
        clash
    }

    /// Every symbol of that name visible here, innermost first.
    pub fn lookup(&self, name: &str) -> &[SymbolId] {
        for frame in self.frames.iter().rev() {
            if let Some(ids) = frame.names.get(name) {
                return ids;
            }
        }
        &[]
    }

    /// The innermost symbol of that name.
    pub fn lookup_one(&self, name: &str) -> Option<SymbolId> {
        self.lookup(name).last().copied()
    }
}
