//! The GLSL type model — GLSL 4.60 §4.1.
//!
//! GLSL's type system is small and closed, so [`Type`] is a closed enum rather
//! than an interned graph. Three decisions shape it:
//!
//! - **`Unknown` is a first-class type, and it is the quiet one.** Anything the
//!   analysis could not work out — a macro-heavy expression, an extension type,
//!   a struct that never parsed — is `Unknown`, and every rule in this crate
//!   treats an `Unknown` operand as "say nothing". A false error in an editor is
//!   worse than a missed one.
//! - **Opaque types stay strings.** There are ~90 sampler and image names, they
//!   have no structure worth modelling, and [`glsl_spec::BASIC_TYPES`] already
//!   owns a `&'static str` for each, so `Opaque` borrows that.
//! - **Structs are ids into one table.** A `Type` has to be `Clone` and cheap;
//!   a struct's fields live in [`StructTable`] and the type is an index.

use glsl_spec::TypeKind;

/// The scalar component types — GLSL 4.60 §4.1.2–§4.1.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scalar {
    Bool,
    Int,
    Uint,
    Float,
    Double,
}

impl Scalar {
    pub const ALL: &'static [Scalar] =
        &[Scalar::Bool, Scalar::Int, Scalar::Uint, Scalar::Float, Scalar::Double];

    pub const fn name(self) -> &'static str {
        match self {
            Scalar::Bool => "bool",
            Scalar::Int => "int",
            Scalar::Uint => "uint",
            Scalar::Float => "float",
            Scalar::Double => "double",
        }
    }

    /// The prefix its vector types carry: `vec3`, `ivec3`, `bvec3`.
    pub const fn vector_prefix(self) -> &'static str {
        match self {
            Scalar::Bool => "bvec",
            Scalar::Int => "ivec",
            Scalar::Uint => "uvec",
            Scalar::Float => "vec",
            Scalar::Double => "dvec",
        }
    }

    /// Whether arithmetic is defined on it — everything but `bool`.
    pub const fn is_numeric(self) -> bool {
        !matches!(self, Scalar::Bool)
    }

    /// Whether the integer operators (`% << >> & | ^ ~`) accept it.
    pub const fn is_integer(self) -> bool {
        matches!(self, Scalar::Int | Scalar::Uint)
    }

    /// Whether it is one of the floating-point types.
    pub const fn is_float(self) -> bool {
        matches!(self, Scalar::Float | Scalar::Double)
    }
}

/// A struct or interface block's index in a [`StructTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructId(pub u32);

impl StructId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A type as an expression or a declaration has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Not worked out. Every rule treats this as "stay silent".
    Unknown,
    Void,
    Scalar(Scalar),
    /// 2, 3 or 4 components of one scalar type.
    Vector(Scalar, u8),
    /// `matCxR` — `cols` columns of `rows` rows, `mat4x2` being 4 columns of 2.
    Matrix { cols: u8, rows: u8, double: bool },
    /// An array, sized or not. An unsized array is one whose size the
    /// declaration left open — a shader-storage tail, or `float x[]` awaiting
    /// its initialiser.
    Array(Box<Type>, Option<u32>),
    Struct(StructId),
    /// A sampler, image or atomic counter, spelled as [`glsl_spec`] spells it.
    Opaque(&'static str),
}

impl Type {
    pub const FLOAT: Type = Type::Scalar(Scalar::Float);
    pub const INT: Type = Type::Scalar(Scalar::Int);
    pub const UINT: Type = Type::Scalar(Scalar::Uint);
    pub const BOOL: Type = Type::Scalar(Scalar::Bool);

    /// The type a basic type name spells, if it spells one.
    ///
    /// Reads [`glsl_spec::basic_type`] first so that only names the language
    /// really has get here, then decomposes the spelling — which is uniform
    /// enough (`[iub d]vec[234]`, `[d]mat[234][x[234]]`) to parse rather than
    /// tabulate.
    pub fn from_name(name: &str) -> Option<Type> {
        let basic = glsl_spec::basic_type(name)?;
        match basic.kind {
            TypeKind::Void => Some(Type::Void),
            TypeKind::Scalar => scalar_named(name).map(Type::Scalar),
            TypeKind::Vector => vector_named(name),
            TypeKind::Matrix => matrix_named(name),
            TypeKind::Sampler | TypeKind::Image | TypeKind::AtomicCounter => {
                Some(Type::Opaque(basic.name))
            }
        }
    }

    pub const fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    /// Whether either side of a rule should make the rule stay quiet.
    pub const fn is_indeterminate(&self) -> bool {
        matches!(self, Type::Unknown | Type::Void)
    }

    /// The scalar every component of this type has, for the types that have
    /// components at all.
    pub const fn component(&self) -> Option<Scalar> {
        match self {
            Type::Scalar(s) | Type::Vector(s, _) => Some(*s),
            Type::Matrix { double, .. } => {
                Some(if *double { Scalar::Double } else { Scalar::Float })
            }
            _ => None,
        }
    }

    /// How many components a scalar, vector or matrix holds.
    pub const fn component_count(&self) -> Option<u32> {
        match self {
            Type::Scalar(_) => Some(1),
            Type::Vector(_, n) => Some(*n as u32),
            Type::Matrix { cols, rows, .. } => Some(*cols as u32 * *rows as u32),
            _ => None,
        }
    }

    pub const fn is_scalar(&self) -> bool {
        matches!(self, Type::Scalar(_))
    }

    pub const fn is_vector(&self) -> bool {
        matches!(self, Type::Vector(..))
    }

    pub const fn is_matrix(&self) -> bool {
        matches!(self, Type::Matrix { .. })
    }

    pub const fn is_opaque(&self) -> bool {
        matches!(self, Type::Opaque(_))
    }

    pub const fn is_array(&self) -> bool {
        matches!(self, Type::Array(..))
    }

    /// Whether every component is numeric — what arithmetic needs.
    pub fn is_numeric(&self) -> bool {
        self.component().is_some_and(Scalar::is_numeric)
    }

    /// Whether every component is an integer — what `%`, the shifts and the
    /// bitwise operators need.
    pub fn is_integral(&self) -> bool {
        matches!(self, Type::Scalar(s) | Type::Vector(s, _) if s.is_integer())
    }

    /// The same shape with a different component type: `ivec3` → `vec3`.
    /// Matrices keep their shape and only their `double`ness can change.
    pub fn with_component(&self, scalar: Scalar) -> Type {
        match self {
            Type::Scalar(_) => Type::Scalar(scalar),
            Type::Vector(_, n) => Type::Vector(scalar, *n),
            Type::Matrix { cols, rows, .. } => {
                Type::Matrix { cols: *cols, rows: *rows, double: scalar == Scalar::Double }
            }
            other => other.clone(),
        }
    }

    /// The type indexing this one yields: a vector's component, a matrix's
    /// column, an array's element.
    pub fn indexed(&self) -> Option<Type> {
        match self {
            Type::Vector(scalar, _) => Some(Type::Scalar(*scalar)),
            Type::Matrix { rows, double, .. } => Some(Type::Vector(
                if *double { Scalar::Double } else { Scalar::Float },
                *rows,
            )),
            Type::Array(element, _) => Some((**element).clone()),
            _ => None,
        }
    }

    /// How many elements an index may address, when that is known.
    pub fn index_bound(&self) -> Option<u32> {
        match self {
            Type::Vector(_, n) => Some(*n as u32),
            Type::Matrix { cols, .. } => Some(*cols as u32),
            Type::Array(_, size) => *size,
            _ => None,
        }
    }

    /// The spelling GLSL uses, which is also what a diagnostic prints.
    pub fn name(&self, structs: &StructTable) -> String {
        match self {
            Type::Unknown => "?".to_string(),
            Type::Void => "void".to_string(),
            Type::Scalar(s) => s.name().to_string(),
            Type::Vector(s, n) => format!("{}{n}", s.vector_prefix()),
            Type::Matrix { cols, rows, double } => {
                let prefix = if *double { "dmat" } else { "mat" };
                if cols == rows {
                    format!("{prefix}{cols}")
                } else {
                    format!("{prefix}{cols}x{rows}")
                }
            }
            Type::Array(element, Some(size)) => format!("{}[{size}]", element.name(structs)),
            Type::Array(element, None) => format!("{}[]", element.name(structs)),
            Type::Struct(id) => structs.name_of(*id).to_string(),
            Type::Opaque(name) => (*name).to_string(),
        }
    }
}

/// One member of a struct or an interface block.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub name_span: analyzer_core::spans::ByteSpan,
}

/// A declared struct or interface block.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    /// Whether this came from an interface block rather than a `struct`.
    pub is_block: bool,
    /// The declaration itself. Registration is keyed on it, so a type
    /// specifier read twice — a function's return type is read once when its
    /// signature is collected and again when its body is walked — registers
    /// one struct rather than two.
    pub decl_span: analyzer_core::spans::ByteSpan,
}

impl StructDef {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// Every struct and interface block a file declares.
#[derive(Debug, Clone, Default)]
pub struct StructTable {
    defs: Vec<StructDef>,
}

impl StructTable {
    pub fn push(&mut self, def: StructDef) -> StructId {
        let id = StructId(self.defs.len() as u32);
        self.defs.push(def);
        id
    }

    pub fn get(&self, id: StructId) -> Option<&StructDef> {
        self.defs.get(id.index())
    }

    pub fn get_mut(&mut self, id: StructId) -> Option<&mut StructDef> {
        self.defs.get_mut(id.index())
    }

    pub fn name_of(&self, id: StructId) -> &str {
        self.get(id).map_or("<anonymous>", |d| d.name.as_str())
    }

    /// The struct already registered for this declaration, if there is one.
    pub fn by_declaration(
        &self,
        span: analyzer_core::spans::ByteSpan,
    ) -> Option<StructId> {
        (!span.is_empty())
            .then(|| {
                self.defs
                    .iter()
                    .position(|d| d.decl_span == span)
                    .map(|i| StructId(i as u32))
            })
            .flatten()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

fn scalar_named(name: &str) -> Option<Scalar> {
    Scalar::ALL.iter().copied().find(|s| s.name() == name)
}

fn vector_named(name: &str) -> Option<Type> {
    for scalar in Scalar::ALL.iter().copied() {
        let prefix = scalar.vector_prefix();
        // `vec` is a suffix of `bvec`/`ivec`/`uvec`/`dvec`, so the prefixed
        // spellings have to be tried first — which `ALL`'s order does, `Float`
        // coming after the three that would otherwise be shadowed. The explicit
        // length check makes that independent of the order anyway.
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.len() == 1 && name.len() == prefix.len() + 1 {
                let n = rest.as_bytes()[0] - b'0';
                if (2..=4).contains(&n) {
                    return Some(Type::Vector(scalar, n));
                }
            }
        }
    }
    None
}

fn matrix_named(name: &str) -> Option<Type> {
    let (double, rest) = match name.strip_prefix("dmat") {
        Some(rest) => (true, rest),
        None => (false, name.strip_prefix("mat")?),
    };
    let digits: Vec<u8> = rest.bytes().collect();
    match digits.as_slice() {
        [n] if (b'2'..=b'4').contains(n) => {
            let n = n - b'0';
            Some(Type::Matrix { cols: n, rows: n, double })
        }
        [c, b'x', r] if (b'2'..=b'4').contains(c) && (b'2'..=b'4').contains(r) => {
            Some(Type::Matrix { cols: c - b'0', rows: r - b'0', double })
        }
        _ => None,
    }
}
