//! P4-02 — the type model: names, equality, shapes, arrays and structs.

use crate::types::{Scalar, StructDef, StructTable, Type};

#[test]
fn every_predeclared_type_name_parses_back_to_itself() {
    // The one gate that matters for the model: `glsl-spec` names ~130 types,
    // and every one of them must survive `name → Type → name` unchanged. A
    // `mat4x2` that comes back as `mat2x4` is a silent wrong answer everywhere
    // else in this crate.
    //
    // The one licensed difference is the square matrices, which GLSL spells
    // two ways — `mat2x2` reads back as `mat2`, the canonical form — so the
    // round-trip is checked on the *type*, and the spelling separately for
    // everything that has only one.
    let structs = StructTable::default();
    for basic in glsl_spec::BASIC_TYPES {
        let ty = Type::from_name(basic.name)
            .unwrap_or_else(|| panic!("{} is not a type this model has", basic.name));
        let printed = ty.name(&structs);
        assert_eq!(
            Type::from_name(&printed),
            Some(ty.clone()),
            "{} printed as {printed}, which is a different type",
            basic.name
        );
        let square = matches!(&ty, Type::Matrix { cols, rows, .. } if cols == rows);
        if !square {
            assert_eq!(printed, basic.name, "{} did not round-trip", basic.name);
        }
    }
}

#[test]
fn a_name_the_language_does_not_have_is_not_a_type() {
    assert_eq!(Type::from_name("vec5"), None);
    assert_eq!(Type::from_name("mat5x2"), None);
    assert_eq!(Type::from_name("Vec3"), None);
    assert_eq!(Type::from_name("float16_t"), None);
    assert_eq!(Type::from_name(""), None);
}

#[test]
fn matrices_are_columns_by_rows() {
    // `mat4x2` is 4 columns of 2 rows — the spelling GLSL uses and the one
    // every rule about matrix multiplication depends on.
    assert_eq!(
        Type::from_name("mat4x2"),
        Some(Type::Matrix { cols: 4, rows: 2, double: false })
    );
    assert_eq!(
        Type::from_name("mat3"),
        Some(Type::Matrix { cols: 3, rows: 3, double: false })
    );
    assert_eq!(
        Type::from_name("dmat2x4"),
        Some(Type::Matrix { cols: 2, rows: 4, double: true })
    );
}

#[test]
fn components_and_shapes() {
    let vec3 = Type::from_name("vec3").unwrap();
    assert_eq!(vec3.component(), Some(Scalar::Float));
    assert_eq!(vec3.component_count(), Some(3));
    assert_eq!(vec3.index_bound(), Some(3));
    assert_eq!(vec3.indexed(), Some(Type::FLOAT));

    let mat4x2 = Type::from_name("mat4x2").unwrap();
    assert_eq!(mat4x2.component_count(), Some(8));
    // Indexing a matrix yields a *column*, which is as long as the matrix has
    // rows, and there are as many of them as it has columns.
    assert_eq!(mat4x2.indexed(), Some(Type::Vector(Scalar::Float, 2)));
    assert_eq!(mat4x2.index_bound(), Some(4));

    assert_eq!(Type::from_name("uvec2").unwrap().with_component(Scalar::Float),
        Type::Vector(Scalar::Float, 2));
}

#[test]
fn arrays_print_and_index_like_arrays() {
    let structs = StructTable::default();
    let sized = Type::Array(Box::new(Type::FLOAT), Some(4));
    assert_eq!(sized.name(&structs), "float[4]");
    assert_eq!(sized.indexed(), Some(Type::FLOAT));
    assert_eq!(sized.index_bound(), Some(4));

    let open = Type::Array(Box::new(Type::from_name("vec2").unwrap()), None);
    assert_eq!(open.name(&structs), "vec2[]");
    // An unsized array can be indexed; nothing is known about the bound.
    assert_eq!(open.index_bound(), None);

    // `float[2][3]` is two arrays of three, and prints that way round.
    let nested = Type::Array(Box::new(sized), Some(2));
    assert_eq!(nested.name(&structs), "float[4][2]");
}

#[test]
fn structs_are_named_through_the_table() {
    let mut structs = StructTable::default();
    let id = structs.push(StructDef {
        name: "Light".to_string(),
        fields: Vec::new(),
        is_block: false,
        decl_span: analyzer_core::spans::ByteSpan::new(0, 1),
    });
    assert_eq!(Type::Struct(id).name(&structs), "Light");
    // A struct is not indexable and has no components, which is what keeps
    // `s[0]` and `s + 1` from being accepted.
    assert_eq!(Type::Struct(id).indexed(), None);
    assert_eq!(Type::Struct(id).component_count(), None);
}

#[test]
fn unknown_is_the_quiet_type() {
    let structs = StructTable::default();
    assert!(Type::Unknown.is_unknown());
    assert!(Type::Unknown.is_indeterminate());
    assert!(Type::Void.is_indeterminate());
    assert_eq!(Type::Unknown.name(&structs), "?");
    assert_eq!(Type::Unknown.component(), None);
}

#[test]
fn the_integer_and_numeric_families() {
    let cases: &[(&str, bool, bool)] = &[
        // name, is_numeric, is_integral
        ("float", true, false),
        ("int", true, true),
        ("uint", true, true),
        ("bool", false, false),
        ("ivec3", true, true),
        ("bvec3", false, false),
        ("mat3", true, false),
        ("sampler2D", false, false),
    ];
    for (name, numeric, integral) in cases {
        let ty = Type::from_name(name).unwrap();
        assert_eq!(ty.is_numeric(), *numeric, "{name}");
        assert_eq!(ty.is_integral(), *integral, "{name}");
    }
}
