//! Rust types as RON sees them.
//!
//! Lowering turns a declared [`TypeExpr`] into a [`Ty`]: aliases expand,
//! generic parameters substitute, and wrappers that serialize as their
//! contents (`Box`, `Arc`, `Cow`, ...) disappear. A type the schema cannot
//! see into — a foreign crate's struct, `serde_json::Value`, a trait
//! object — lowers to [`Ty::Any`], which accepts everything; the schema
//! would rather miss an error than invent one.

use crate::rust::parser::parse_type_text;
use crate::rust::syntax::{Field, ItemKind, TypeExpr};
use crate::rust::world::{ItemRef, ModPath, Res, World};
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntKind {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

impl IntKind {
    pub fn from_name(name: &str) -> Option<IntKind> {
        Some(match name {
            "i8" => IntKind::I8,
            "i16" => IntKind::I16,
            "i32" => IntKind::I32,
            "i64" => IntKind::I64,
            "i128" => IntKind::I128,
            "isize" => IntKind::Isize,
            "u8" => IntKind::U8,
            "u16" => IntKind::U16,
            "u32" => IntKind::U32,
            "u64" => IntKind::U64,
            "u128" => IntKind::U128,
            "usize" => IntKind::Usize,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            IntKind::I8 => "i8",
            IntKind::I16 => "i16",
            IntKind::I32 => "i32",
            IntKind::I64 => "i64",
            IntKind::I128 => "i128",
            IntKind::Isize => "isize",
            IntKind::U8 => "u8",
            IntKind::U16 => "u16",
            IntKind::U32 => "u32",
            IntKind::U64 => "u64",
            IntKind::U128 => "u128",
            IntKind::Usize => "usize",
        }
    }

    pub fn signed(self) -> bool {
        matches!(self, IntKind::I8 | IntKind::I16 | IntKind::I32 | IntKind::I64 | IntKind::I128 | IntKind::Isize)
    }

    pub fn bits(self) -> u32 {
        match self {
            IntKind::I8 | IntKind::U8 => 8,
            IntKind::I16 | IntKind::U16 => 16,
            IntKind::I32 | IntKind::U32 => 32,
            IntKind::I64 | IntKind::U64 | IntKind::Isize | IntKind::Usize => 64,
            IntKind::I128 | IntKind::U128 => 128,
        }
    }

    /// Whether `-magnitude` (when `negative`) or `magnitude` fits.
    pub fn fits(self, negative: bool, magnitude: u128) -> bool {
        let bits = self.bits();
        if self.signed() {
            let limit = 1u128 << (bits - 1);
            if negative { magnitude <= limit } else { magnitude < limit }
        } else if negative {
            magnitude == 0
        } else {
            bits == 128 || magnitude < (1u128 << bits)
        }
    }
}

#[derive(Debug, Clone)]
pub enum Ty {
    /// Unknown or opaque: accepts anything.
    Any,
    Unit,
    Bool,
    Int(IntKind),
    /// `f32` or `f64`.
    Float(&'static str),
    Char,
    /// `String`, `&str`, `PathBuf`, and anything else serialized as text.
    Str,
    Option(Box<Ty>),
    /// A homogeneous sequence; the name is the collection as declared
    /// (`Vec`, `HashSet`, ...), for messages.
    Seq(&'static str, Box<Ty>),
    /// `[T; N]`, which RON writes as a tuple (or a list).
    Array(Box<Ty>, Option<u64>),
    Map(&'static str, Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    /// A struct or enum found in the workspace.
    Adt(Rc<Adt>),
}

#[derive(Debug)]
pub struct Adt {
    pub item: ItemRef,
    pub args: Vec<Ty>,
}

impl Ty {
    pub fn is_option(&self) -> bool {
        matches!(self, Ty::Option(_))
    }

    pub fn adt(&self) -> Option<&Rc<Adt>> {
        match self {
            Ty::Adt(a) => Some(a),
            _ => None,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Any => write!(f, "_"),
            Ty::Unit => write!(f, "()"),
            Ty::Bool => write!(f, "bool"),
            Ty::Int(k) => write!(f, "{}", k.name()),
            Ty::Float(n) => write!(f, "{n}"),
            Ty::Char => write!(f, "char"),
            Ty::Str => write!(f, "String"),
            Ty::Option(t) => write!(f, "Option<{t}>"),
            Ty::Seq("slice", t) => write!(f, "[{t}]"),
            Ty::Seq(n, t) => write!(f, "{n}<{t}>"),
            Ty::Array(t, Some(n)) => write!(f, "[{t}; {n}]"),
            Ty::Array(t, None) => write!(f, "[{t}; _]"),
            Ty::Map(n, k, v) => write!(f, "{n}<{k}, {v}>"),
            Ty::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                if ts.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Ty::Adt(a) => {
                write!(f, "{}", a.item.item().name())?;
                if !a.args.is_empty() {
                    write!(f, "<")?;
                    for (i, t) in a.args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{t}")?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
        }
    }
}

type Env = Vec<(String, Ty)>;

/// Turns declared types into [`Ty`] against a [`World`].
pub struct Lowerer<'w> {
    pub world: &'w World,
}

impl<'w> Lowerer<'w> {
    pub fn new(world: &'w World) -> Self {
        Lowerer { world }
    }

    /// Lower `ty` as written in module `module`, with generic parameters
    /// bound by `env`.
    pub fn lower(&self, ty: &TypeExpr, module: &ModPath, env: &Env) -> Ty {
        self.lower_depth(ty, module, env, 0)
    }

    fn lower_depth(&self, ty: &TypeExpr, module: &ModPath, env: &Env, depth: u32) -> Ty {
        if depth > 16 {
            return Ty::Any;
        }
        match ty {
            TypeExpr::Path { global, segments } => {
                let last = segments.last().expect("a path has segments");
                let args: Vec<Ty> = last.args.iter().map(|a| self.lower_depth(a, module, env, depth + 1)).collect();
                if !global && segments.len() == 1 {
                    if let Some((_, bound)) = env.iter().find(|(n, _)| *n == last.name) {
                        return bound.clone();
                    }
                    if let Some(p) = primitive(&last.name) {
                        return p;
                    }
                }
                let names: Vec<String> = segments.iter().map(|s| s.name.clone()).collect();
                let res = self.world.scope(module).and_then(|s| self.world.resolve_path(&s, *global, &names));
                match res {
                    Some(Res::Item(item)) => self.instantiate(item, args, depth),
                    Some(Res::External(_)) | None => builtin(&last.name, args).unwrap_or(Ty::Any),
                    Some(Res::Module(_)) => Ty::Any,
                }
            }
            TypeExpr::Tuple(elems) if elems.is_empty() => Ty::Unit,
            TypeExpr::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.lower_depth(e, module, env, depth + 1)).collect()),
            TypeExpr::Array(elem, n) => Ty::Array(Box::new(self.lower_depth(elem, module, env, depth + 1)), *n),
            TypeExpr::Slice(elem) => Ty::Seq("slice", Box::new(self.lower_depth(elem, module, env, depth + 1))),
            TypeExpr::Ref(inner) => self.lower_depth(inner, module, env, depth + 1),
            TypeExpr::Other => Ty::Any,
        }
    }

    /// A resolved item with its generic arguments: an ADT, or an alias
    /// expanded in place.
    pub fn instantiate(&self, item: ItemRef, args: Vec<Ty>, depth: u32) -> Ty {
        let def = item.item();
        match &def.kind {
            ItemKind::Struct { .. } | ItemKind::Enum { .. } => Ty::Adt(Rc::new(Adt { item, args })),
            ItemKind::TypeAlias { generics, ty } => {
                let env = bind(generics, &args);
                self.lower_depth(ty, &item.module, &env, depth + 1)
            }
            _ => Ty::Any,
        }
    }

    /// Generic parameters of `adt`'s declaration bound to its arguments.
    pub fn env(&self, adt: &Adt) -> Env {
        bind(adt.item.item().generics(), &adt.args)
    }

    /// The type a field deserializes as.
    pub fn field_ty(&self, adt: &Adt, field: &Field) -> Ty {
        if field.serde.custom {
            return Ty::Any;
        }
        self.lower(&field.ty, &adt.item.module, &self.env(adt))
    }

    /// `#[serde(from = "T")]` / `try_from`: the type actually read.
    pub fn from_ty(&self, adt: &Adt) -> Option<Ty> {
        let text = adt.item.item().serde.from.as_ref()?;
        let te = parse_type_text(text)?;
        Some(self.lower(&te, &adt.item.module, &self.env(adt)))
    }
}

fn bind(generics: &[String], args: &[Ty]) -> Env {
    generics.iter().enumerate().map(|(i, g)| (g.clone(), args.get(i).cloned().unwrap_or(Ty::Any))).collect()
}

fn primitive(name: &str) -> Option<Ty> {
    if let Some(k) = IntKind::from_name(name) {
        return Some(Ty::Int(k));
    }
    Some(match name {
        "bool" => Ty::Bool,
        "f32" => Ty::Float("f32"),
        "f64" => Ty::Float("f64"),
        "char" => Ty::Char,
        "str" => Ty::Str,
        _ => return None,
    })
}

/// Well-known types from `std` and popular crates, by their last path
/// segment.
fn builtin(name: &str, mut args: Vec<Ty>) -> Option<Ty> {
    if let Some(p) = primitive(name) {
        return Some(p);
    }
    let mut arg = |i: usize| -> Ty { if i < args.len() { std::mem::replace(&mut args[i], Ty::Any) } else { Ty::Any } };
    let seq = |n: &'static str, t: Ty| Ty::Seq(n, Box::new(t));
    Some(match name {
        "String" | "PathBuf" | "Path" | "OsString" | "OsStr" | "CString" | "CStr" | "SmolStr" | "CompactString"
        | "ArcStr" | "Utf8PathBuf" | "Utf8Path" | "Ipv4Addr" | "Ipv6Addr" | "IpAddr" | "SocketAddr"
        | "SocketAddrV4" | "SocketAddrV6" | "Uuid" | "Url" | "DateTime" | "NaiveDate" | "NaiveDateTime"
        | "NaiveTime" => Ty::Str,
        "Option" => Ty::Option(Box::new(arg(0))),
        "Vec" => seq("Vec", arg(0)),
        "VecDeque" => seq("VecDeque", arg(0)),
        "LinkedList" => seq("LinkedList", arg(0)),
        "BinaryHeap" => seq("BinaryHeap", arg(0)),
        "HashSet" => seq("HashSet", arg(0)),
        "BTreeSet" => seq("BTreeSet", arg(0)),
        "IndexSet" => seq("IndexSet", arg(0)),
        "FxHashSet" => seq("FxHashSet", arg(0)),
        "AHashSet" => seq("AHashSet", arg(0)),
        "ThinVec" => seq("ThinVec", arg(0)),
        "SmallVec" | "ArrayVec" | "TinyVec" => {
            let n: &'static str = match name {
                "SmallVec" => "SmallVec",
                "ArrayVec" => "ArrayVec",
                _ => "TinyVec",
            };
            match arg(0) {
                Ty::Array(t, _) => Ty::Seq(n, t),
                t => seq(n, t),
            }
        }
        "HashMap" | "BTreeMap" | "IndexMap" | "FxHashMap" | "AHashMap" | "DashMap" => {
            let n: &'static str = match name {
                "HashMap" => "HashMap",
                "BTreeMap" => "BTreeMap",
                "IndexMap" => "IndexMap",
                "FxHashMap" => "FxHashMap",
                "AHashMap" => "AHashMap",
                _ => "DashMap",
            };
            let k = arg(0);
            Ty::Map(n, Box::new(k), Box::new(arg(1)))
        }
        "Box" | "Rc" | "Arc" | "Cow" | "RefCell" | "Cell" | "Mutex" | "RwLock" | "Reverse" | "Wrapping"
        | "Saturating" | "OrderedFloat" | "NotNan" | "NonZero" | "OnceCell" => arg(0),
        "PhantomData" | "PhantomPinned" => Ty::Unit,
        "AtomicBool" => Ty::Bool,
        _ => {
            let int = name.strip_prefix("NonZero").or_else(|| name.strip_prefix("Atomic"))?;
            Ty::Int(IntKind::from_name(&int.to_ascii_lowercase())?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::world::tests::world;

    fn lower_field(files: &[(&str, &str)], ty_name: &str, field: usize) -> String {
        let w = world(files);
        let krate = w.crates_for("file:///p/a.ron").remove(0);
        let scope = w.root_scope(krate).unwrap();
        let Some(Res::Item(item)) = w.resolve_path(&scope, false, &[ty_name.to_string()]) else { panic!("unresolved") };
        let l = Lowerer::new(&w);
        let Ty::Adt(adt) = l.instantiate(item, vec![], 0) else { panic!("not an adt") };
        let ItemKind::Struct { fields, .. } = &adt.item.item().kind else { panic!() };
        l.field_ty(&adt, &fields.list()[field]).to_string()
    }

    const FILES: &[(&str, &str)] = &[
        ("file:///p/Cargo.toml", "[package]\nname = \"p\"\n"),
        (
            "file:///p/src/lib.rs",
            "use std::collections::HashMap;\n\
             type Grid<T> = Vec<Vec<T>>;\n\
             pub struct Wrapper<T> { inner: T }\n\
             pub struct Cell;\n\
             pub struct S {\n\
               a: Option<Box<str>>,\n\
               b: HashMap<String, std::sync::Arc<Cell>>,\n\
               c: Grid<u8>,\n\
               d: Wrapper<f32>,\n\
               e: [(i64, char); 2],\n\
               f: smallvec::SmallVec<[u16; 4]>,\n\
               g: serde_json::Value,\n\
               h: &'static std::num::NonZeroU32,\n\
             }\n",
        ),
    ];

    #[test]
    fn lowering() {
        let got: Vec<String> = (0..8).map(|i| lower_field(FILES, "S", i)).collect();
        assert_eq!(
            got,
            [
                "Option<String>",
                "HashMap<String, Cell>",
                "Vec<Vec<u8>>",
                "Wrapper<f32>",
                "[(i64, char); 2]",
                "SmallVec<u16>",
                "_",
                "u32",
            ]
        );
    }

    #[test]
    fn int_ranges() {
        assert!(IntKind::U8.fits(false, 255));
        assert!(!IntKind::U8.fits(false, 256));
        assert!(!IntKind::U8.fits(true, 1));
        assert!(IntKind::I8.fits(true, 128));
        assert!(!IntKind::I8.fits(false, 128));
        assert!(IntKind::U128.fits(false, u128::MAX));
    }
}
