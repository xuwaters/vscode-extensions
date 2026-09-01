//! Implicit conversions (§4.1.10) and constructor legality (§5.4).
//!
//! The conversion table is tiny and closed:
//!
//! ```text
//! int  → uint, float, double
//! uint →       float, double
//! float →             double
//! ```
//!
//! plus the same conversions applied component-wise to vectors of matching
//! size, and `mat` → `dmat` of matching shape. `bool` converts to nothing and
//! nothing converts to `bool`; that is the rule behind most of the honest
//! errors this crate reports.
//!
//! Overload resolution needs more than yes/no, so [`conversion_cost`] ranks the
//! conversions the way §6.1 ranks them: an exact match beats a promotion to a
//! type of the same width, which beats a widening to `double`. The numbers are
//! ordinal only — nothing outside this module reads them as anything but
//! "smaller is better".

use crate::types::{Scalar, StructTable, Type};

/// The cost of an exact match: the parameter type *is* the argument type.
pub const EXACT: u32 = 0;

/// Whether `from` converts to `to` with no cast written.
pub fn implicitly_convertible(from: &Type, to: &Type) -> bool {
    conversion_cost(from, to).is_some()
}

/// How good a match `from` → `to` is, `None` when it is not a match at all.
///
/// The ordering is §6.1's: exact, then a conversion that keeps the width
/// (`int` → `uint`), then one that widens to floating point, then one that
/// widens to `double`. Vectors and matrices cost what their components cost, so
/// `ivec3` → `vec3` ranks exactly like `int` → `float`.
pub fn conversion_cost(from: &Type, to: &Type) -> Option<u32> {
    if from == to {
        return Some(EXACT);
    }
    // An unknown operand matches anything, at the worst possible rank — the
    // call still resolves to *something* for hover, and no diagnostic is built
    // on a cost this module invented.
    if from.is_unknown() || to.is_unknown() {
        return Some(64);
    }
    match (from, to) {
        (Type::Scalar(a), Type::Scalar(b)) => scalar_cost(*a, *b),
        (Type::Vector(a, n), Type::Vector(b, m)) if n == m => scalar_cost(*a, *b),
        (
            Type::Matrix { cols: c1, rows: r1, double: d1 },
            Type::Matrix { cols: c2, rows: r2, double: d2 },
        ) if c1 == c2 && r1 == r2 => match (d1, d2) {
            (false, true) => Some(3),
            _ => None,
        },
        (Type::Array(a, sa), Type::Array(b, sb)) if a == b => {
            // An unsized parameter accepts a sized argument: that is how an
            // unsized array parameter and a runtime-sized buffer member work.
            match (sa, sb) {
                (_, None) => Some(1),
                (Some(x), Some(y)) if x == y => Some(EXACT),
                _ => None,
            }
        }
        _ => None,
    }
}

/// §4.1.10's table, as ranks.
fn scalar_cost(from: Scalar, to: Scalar) -> Option<u32> {
    if from == to {
        return Some(EXACT);
    }
    match (from, to) {
        (Scalar::Int, Scalar::Uint) => Some(1),
        (Scalar::Int | Scalar::Uint, Scalar::Float) => Some(2),
        (Scalar::Int | Scalar::Uint | Scalar::Float, Scalar::Double) => Some(3),
        _ => None,
    }
}

/// The type two operands of a component-wise operator both convert to, if
/// there is one. `float + int` is `float`; `int + uint` is `uint`; `bool + int`
/// is nothing at all.
pub fn common_type(left: &Type, right: &Type) -> Option<Type> {
    if left == right {
        return Some(left.clone());
    }
    match (conversion_cost(left, right), conversion_cost(right, left)) {
        (Some(a), Some(b)) => Some(if a <= b { right.clone() } else { left.clone() }),
        (Some(_), None) => Some(right.clone()),
        (None, Some(_)) => Some(left.clone()),
        (None, None) => None,
    }
}

/// What a constructor call amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constructed {
    /// The arguments build the type.
    Ok,
    /// They do not, and here is why, phrased for the editor.
    Rejected(String),
    /// Not enough is known to say. Never a diagnostic.
    Unsure,
}

/// Whether `args` can construct `target` — GLSL 4.60 §5.4.
///
/// The rules, in the order they are applied:
///
/// 1. Any unknown argument, or an unknown target, means silence.
/// 2. A scalar takes exactly one scalar argument, of any scalar type. This is
///    where explicit conversion lives: `int(bool)` and `bool(float)` are legal
///    even though neither converts implicitly.
/// 3. A vector or matrix built from one scalar is a splat or a diagonal.
/// 4. A matrix built from one matrix is a resize, whatever the shapes.
/// 5. Otherwise the arguments' components are laid out in order, and there must
///    be enough of them. An argument that contributes nothing — every one of
///    its components past the end — is an error; extra components at the tail
///    of the last argument are dropped, which the language allows.
/// 6. A struct takes one argument per field, each convertible. An array takes
///    one per element.
pub fn construct(target: &Type, args: &[Type], structs: &StructTable) -> Constructed {
    if target.is_unknown() || args.iter().any(Type::is_unknown) {
        return Constructed::Unsure;
    }
    match target {
        Type::Void => Constructed::Rejected("'void' has no constructor".to_string()),
        Type::Opaque(name) => {
            Constructed::Rejected(format!("'{name}' cannot be constructed; it is an opaque type"))
        }
        Type::Struct(id) => construct_struct(*id, args, structs),
        Type::Array(element, size) => construct_array(element, *size, args, structs),
        Type::Scalar(_) => construct_scalar(target, args, structs),
        Type::Vector(..) | Type::Matrix { .. } => construct_aggregate(target, args, structs),
        Type::Unknown => Constructed::Unsure,
    }
}

fn construct_scalar(target: &Type, args: &[Type], structs: &StructTable) -> Constructed {
    let name = target.name(structs);
    match args {
        [] => Constructed::Rejected(format!("'{name}(…)' needs one argument")),
        [only] => {
            if only.component().is_some() {
                Constructed::Ok
            } else {
                Constructed::Rejected(format!(
                    "'{name}(…)' cannot be built from a {}",
                    only.name(structs)
                ))
            }
        }
        _ => Constructed::Rejected(format!(
            "'{name}(…)' takes one argument, not {}",
            args.len()
        )),
    }
}

fn construct_aggregate(target: &Type, args: &[Type], structs: &StructTable) -> Constructed {
    let name = target.name(structs);
    let Some(wanted) = target.component_count() else {
        return Constructed::Unsure;
    };
    if args.is_empty() {
        return Constructed::Rejected(format!("'{name}(…)' needs at least one argument"));
    }
    // A single scalar fills a vector and the diagonal of a matrix.
    if args.len() == 1 && args[0].is_scalar() {
        return match args[0].component() {
            Some(scalar) if scalar.is_numeric() || target.component() == Some(Scalar::Bool) => {
                Constructed::Ok
            }
            Some(_) => Constructed::Ok,
            None => Constructed::Unsure,
        };
    }
    // A matrix from a matrix keeps what overlaps and fills the rest from the
    // identity, whatever the two shapes are.
    if target.is_matrix() && args.len() == 1 && args[0].is_matrix() {
        return Constructed::Ok;
    }
    if target.is_matrix() && args.iter().any(Type::is_matrix) {
        return Constructed::Rejected(format!(
            "'{name}(…)' takes either one matrix or a list of components, not both"
        ));
    }
    let mut have = 0u32;
    for (i, arg) in args.iter().enumerate() {
        let Some(count) = arg.component_count() else {
            return Constructed::Rejected(format!(
                "a {} cannot be part of a '{name}'",
                arg.name(structs)
            ));
        };
        if have >= wanted {
            return Constructed::Rejected(format!(
                "'{name}' is already complete after {i} argument{}; this one is unused",
                if i == 1 { "" } else { "s" }
            ));
        }
        have += count;
    }
    if have < wanted {
        return Constructed::Rejected(format!(
            "'{name}' needs {wanted} components and these arguments supply {have}"
        ));
    }
    Constructed::Ok
}

fn construct_struct(
    id: crate::types::StructId,
    args: &[Type],
    structs: &StructTable,
) -> Constructed {
    let Some(def) = structs.get(id) else {
        return Constructed::Unsure;
    };
    if def.fields.len() != args.len() {
        return Constructed::Rejected(format!(
            "'{}' has {} member{} and this passes {}",
            def.name,
            def.fields.len(),
            if def.fields.len() == 1 { "" } else { "s" },
            args.len()
        ));
    }
    for (field, arg) in def.fields.iter().zip(args) {
        if field.ty.is_unknown() {
            continue;
        }
        if !implicitly_convertible(arg, &field.ty) {
            return Constructed::Rejected(format!(
                "'{}' is a {}, and a {} does not convert to it",
                field.name,
                field.ty.name(structs),
                arg.name(structs)
            ));
        }
    }
    Constructed::Ok
}

fn construct_array(
    element: &Type,
    size: Option<u32>,
    args: &[Type],
    structs: &StructTable,
) -> Constructed {
    if let Some(size) = size {
        if size as usize != args.len() {
            return Constructed::Rejected(format!(
                "this array has {size} element{} and the constructor passes {}",
                if size == 1 { "" } else { "s" },
                args.len()
            ));
        }
    }
    if element.is_unknown() {
        return Constructed::Unsure;
    }
    for arg in args {
        if !implicitly_convertible(arg, element) {
            return Constructed::Rejected(format!(
                "the elements are {} and a {} does not convert to one",
                element.name(structs),
                arg.name(structs)
            ));
        }
    }
    Constructed::Ok
}
