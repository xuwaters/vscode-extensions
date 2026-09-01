//! P4-03 — implicit conversions (§4.1.10) and constructors (§5.4), rule by
//! rule.

use crate::conversions::{Constructed, common_type, construct, conversion_cost,
    implicitly_convertible};
use crate::tests::{expect, expect_clean};
use crate::types::{StructTable, Type};

fn ty(name: &str) -> Type {
    Type::from_name(name).unwrap_or_else(|| panic!("{name} is not a type"))
}

#[test]
fn the_conversion_table_is_exactly_the_spec_s() {
    // §4.1.10 in full: these convert, and nothing else does.
    let allowed: &[(&str, &str)] = &[
        ("int", "uint"),
        ("int", "float"),
        ("int", "double"),
        ("uint", "float"),
        ("uint", "double"),
        ("float", "double"),
    ];
    for (from, to) in allowed {
        assert!(
            implicitly_convertible(&ty(from), &ty(to)),
            "{from} should convert to {to}"
        );
        // Conversion is one-way. Nothing narrows implicitly.
        assert!(
            !implicitly_convertible(&ty(to), &ty(from)),
            "{to} must not convert back to {from}"
        );
    }
}

#[test]
fn bool_converts_to_nothing_and_nothing_converts_to_bool() {
    for other in ["int", "uint", "float", "double"] {
        assert!(!implicitly_convertible(&ty("bool"), &ty(other)));
        assert!(!implicitly_convertible(&ty(other), &ty("bool")));
    }
    assert!(!implicitly_convertible(&ty("bvec3"), &ty("ivec3")));
}

#[test]
fn vectors_and_matrices_convert_component_wise_at_the_same_size() {
    assert!(implicitly_convertible(&ty("ivec3"), &ty("vec3")));
    assert!(implicitly_convertible(&ty("uvec4"), &ty("dvec4")));
    assert!(implicitly_convertible(&ty("mat2x3"), &ty("dmat2x3")));
    // Different sizes never convert, however compatible the components are.
    assert!(!implicitly_convertible(&ty("ivec3"), &ty("vec4")));
    assert!(!implicitly_convertible(&ty("mat2x3"), &ty("dmat3x2")));
    // Nor does a scalar become a vector without a constructor.
    assert!(!implicitly_convertible(&ty("float"), &ty("vec3")));
}

#[test]
fn conversions_rank_the_way_overload_resolution_needs() {
    // §6.1: an exact match beats a same-width conversion beats a widening.
    let exact = conversion_cost(&ty("float"), &ty("float")).unwrap();
    let to_uint = conversion_cost(&ty("int"), &ty("uint")).unwrap();
    let to_float = conversion_cost(&ty("int"), &ty("float")).unwrap();
    let to_double = conversion_cost(&ty("int"), &ty("double")).unwrap();
    assert!(exact < to_uint, "an exact match must beat every conversion");
    assert!(to_uint < to_float, "int→uint must beat int→float");
    assert!(to_float < to_double, "int→float must beat int→double");
    // The vector form ranks exactly like its component form.
    assert_eq!(conversion_cost(&ty("ivec3"), &ty("vec3")), Some(to_float));
}

#[test]
fn an_unknown_operand_matches_anything_at_the_worst_rank() {
    // This is what keeps an extension type from turning every call it appears
    // in into a "no matching overload".
    let cost = conversion_cost(&Type::Unknown, &ty("vec3")).unwrap();
    assert!(cost > conversion_cost(&ty("int"), &ty("double")).unwrap());
    assert!(implicitly_convertible(&ty("sampler2D"), &Type::Unknown));
}

#[test]
fn the_common_type_of_two_operands() {
    assert_eq!(common_type(&ty("float"), &ty("int")), Some(ty("float")));
    assert_eq!(common_type(&ty("int"), &ty("uint")), Some(ty("uint")));
    assert_eq!(common_type(&ty("vec3"), &ty("dvec3")), Some(ty("dvec3")));
    assert_eq!(common_type(&ty("bool"), &ty("int")), None);
    assert_eq!(common_type(&ty("vec2"), &ty("vec3")), None);
}

// ── Constructors, §5.4 ────────────────────────────────────────────────────

#[track_caller]
fn constructs(target: &str, args: &[&str]) -> Constructed {
    let structs = StructTable::default();
    let args: Vec<Type> = args.iter().map(|a| ty(a)).collect();
    construct(&ty(target), &args, &structs)
}

#[test]
fn a_scalar_constructor_converts_anything_scalar() {
    // This is where the *explicit* conversions live: `int(bool)` is legal
    // where `bool → int` is not.
    assert_eq!(constructs("int", &["bool"]), Constructed::Ok);
    assert_eq!(constructs("bool", &["float"]), Constructed::Ok);
    assert_eq!(constructs("float", &["double"]), Constructed::Ok);
    assert!(matches!(constructs("float", &[]), Constructed::Rejected(_)));
    assert!(matches!(constructs("float", &["float", "float"]), Constructed::Rejected(_)));
    assert!(matches!(constructs("float", &["sampler2D"]), Constructed::Rejected(_)));
}

#[test]
fn one_scalar_splats_a_vector_and_fills_a_matrix_diagonal() {
    assert_eq!(constructs("vec4", &["float"]), Constructed::Ok);
    assert_eq!(constructs("mat4", &["float"]), Constructed::Ok);
    assert_eq!(constructs("bvec2", &["bool"]), Constructed::Ok);
}

#[test]
fn a_vector_needs_enough_components_and_no_wasted_argument() {
    assert_eq!(constructs("vec4", &["vec3", "float"]), Constructed::Ok);
    assert_eq!(constructs("vec4", &["vec2", "vec2"]), Constructed::Ok);
    // Extra components at the tail of the last argument are dropped…
    assert_eq!(constructs("vec2", &["vec4"]), Constructed::Ok);
    // …but an argument that contributes nothing at all is an error.
    assert!(matches!(constructs("vec2", &["vec2", "float"]), Constructed::Rejected(_)));
    assert!(matches!(constructs("vec4", &["vec2"]), Constructed::Rejected(_)));
    assert!(matches!(constructs("vec3", &["float", "float"]), Constructed::Rejected(_)));
}

#[test]
fn a_matrix_takes_a_matrix_or_components_but_not_both() {
    assert_eq!(constructs("mat3", &["mat4"]), Constructed::Ok);
    assert_eq!(constructs("mat2", &["vec2", "vec2"]), Constructed::Ok);
    assert!(matches!(constructs("mat4", &["mat3", "float"]), Constructed::Rejected(_)));
    assert!(matches!(constructs("mat4", &["vec4", "vec4"]), Constructed::Rejected(_)));
}

#[test]
fn an_opaque_type_has_no_constructor() {
    assert!(matches!(constructs("sampler2D", &["int"]), Constructed::Rejected(_)));
    assert!(matches!(constructs("void", &[]), Constructed::Rejected(_)));
}

#[test]
fn an_unknown_argument_makes_the_whole_question_unsure() {
    let structs = StructTable::default();
    assert_eq!(
        construct(&ty("vec4"), &[Type::Unknown], &structs),
        Constructed::Unsure
    );
}

// ── The same rules, through real sources ──────────────────────────────────

#[test]
fn constructor_fixtures() {
    expect_clean(
        "void main() {\n  vec4 a = vec4(1.0);\n  vec4 b = vec4(a.xyz, 1.0);\n\
         \x20 mat3 m = mat3(1.0);\n  ivec2 i = ivec2(a.xy);\n}\n",
    );
    expect(
        "void main() { vec3 v = vec3(1.0, 2.0); }\n",
        &["GLSL0209"],
    );
    expect(
        "void main() { vec2 v = vec2(1.0, 2.0, 3.0); }\n",
        &["GLSL0209"],
    );
}

#[test]
fn struct_and_array_constructors() {
    expect_clean(
        "struct Light { vec3 position; float power; };\n\
         void main() { Light l = Light(vec3(0.0), 1.0); }\n",
    );
    expect(
        "struct Light { vec3 position; float power; };\n\
         void main() { Light l = Light(vec3(0.0)); }\n",
        &["GLSL0209"],
    );
    expect_clean("void main() { float a[3] = float[3](1.0, 2.0, 3.0); }\n");
    expect_clean("void main() { float a[] = float[](1.0, 2.0); }\n");
    expect(
        "void main() { float a[3] = float[3](1.0, 2.0); }\n",
        &["GLSL0209"],
    );
}
