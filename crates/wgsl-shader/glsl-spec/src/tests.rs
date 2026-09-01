//! Tests for the data model and gates on the generated tables.
//!
//! The gates are the interesting half: they assert the things a regeneration
//! could silently break — that `mix` still has its nine signatures, that
//! `texture` still knows ES from desktop, that the doc pool's offsets still
//! land on character boundaries — and the budgets RFC 012 §8 sets.

use crate::model::Stage;
use crate::version::{DesktopVersion, EsVersion, Version};
use crate::*;

// ── The model ─────────────────────────────────────────────────────────────

#[test]
fn version_numbers_round_trip() {
    for &version in DesktopVersion::ALL {
        assert_eq!(DesktopVersion::from_number(version.number()), Some(version));
    }
    for &version in EsVersion::ALL {
        assert_eq!(EsVersion::from_number(version.number()), Some(version));
    }
    // The desktop table has no 1.00 and the ES table has no 4.50; a `#version`
    // number that belongs to the other profile must not resolve.
    assert_eq!(DesktopVersion::from_number(300), None);
    assert_eq!(EsVersion::from_number(450), None);
}

#[test]
fn version_directive_picks_the_profile() {
    assert_eq!(
        Version::from_directive(450, false),
        Some(Version::Desktop(DesktopVersion::V450))
    );
    assert_eq!(Version::from_directive(300, true), Some(Version::Es(EsVersion::V300)));
    // `#version 100` is ES whether or not the profile token is there — desktop
    // GLSL never had a 1.00.
    assert_eq!(Version::from_directive(100, false), Some(Version::Es(EsVersion::V100)));
    assert_eq!(Version::from_directive(999, false), None);
    assert_eq!(Version::DEFAULT, Version::Desktop(DesktopVersion::V110));
}

#[test]
fn masks_are_sets_not_ranges() {
    let mask = DesktopMask::EMPTY.with(DesktopVersion::V110).with(DesktopVersion::V450);
    assert!(mask.contains(DesktopVersion::V110));
    assert!(!mask.contains(DesktopVersion::V330));
    assert_eq!(mask.earliest(), Some(DesktopVersion::V110));
    assert_eq!(mask.latest(), Some(DesktopVersion::V450));
    assert_eq!(mask.versions().count(), 2);
    // A hole in the middle is exactly what `is_contiguous_to_latest` is for.
    assert!(!mask.is_contiguous_to_latest());
    assert!(DesktopMask::since(DesktopVersion::V400).is_contiguous_to_latest());
    assert!(DesktopMask::ALL.is_contiguous_to_latest());
    assert!(!DesktopMask::EMPTY.is_contiguous_to_latest());
}

#[test]
fn mask_constructors_agree() {
    let since = DesktopMask::since(DesktopVersion::V430);
    assert!(since.contains(DesktopVersion::V430));
    assert!(since.contains(DesktopVersion::V460));
    assert!(!since.contains(DesktopVersion::V420));

    let through = DesktopMask::through(DesktopVersion::V110, DesktopVersion::V130);
    assert_eq!(through.versions().count(), 3);
    assert!(through.contains(DesktopVersion::V130));
    assert!(!through.contains(DesktopVersion::V140));

    let only = EsMask::only(EsVersion::V100);
    assert_eq!(only.versions().collect::<Vec<_>>(), vec![EsVersion::V100]);
    assert_eq!(EsMask::ALL.versions().count(), EsVersion::ALL.len());
}

#[test]
fn availability_asks_the_right_profile() {
    let availability =
        Availability::new(DesktopMask::since(DesktopVersion::V400), EsMask::EMPTY);
    assert!(availability.contains(Version::Desktop(DesktopVersion::V450)));
    assert!(!availability.contains(Version::Desktop(DesktopVersion::V330)));
    assert!(!availability.contains(Version::Es(EsVersion::V320)));
    assert!(Availability::NOWHERE.is_empty());
}

#[test]
fn stage_masks_round_trip() {
    let mask = StageMask::EMPTY.with(Stage::Fragment).with(Stage::Compute);
    assert!(mask.contains(Stage::Fragment));
    assert!(!mask.contains(Stage::Vertex));
    assert_eq!(mask.stages().collect::<Vec<_>>(), vec![Stage::Fragment, Stage::Compute]);
    assert_eq!(StageMask::ALL.stages().count(), Stage::ALL.len());
}

#[test]
fn tables_are_sorted_so_lookup_can_binary_search() {
    assert!(FUNCTIONS.windows(2).all(|w| w[0].name < w[1].name), "FUNCTIONS not sorted");
    assert!(VARIABLES.windows(2).all(|w| w[0].name < w[1].name), "VARIABLES not sorted");
    assert!(FAMILIES.windows(2).all(|w| w[0].name < w[1].name), "FAMILIES not sorted");
    assert!(
        RESERVED_KEYWORDS.windows(2).all(|w| w[0] < w[1]),
        "RESERVED_KEYWORDS not sorted"
    );
    for entry in FUNCTIONS {
        assert!(
            entry.param_docs.windows(2).all(|w| w[0].name < w[1].name),
            "{}'s param docs are not sorted",
            entry.name
        );
    }
}

#[test]
fn lookup_finds_what_is_there_and_nothing_else() {
    assert_eq!(function("texture").map(|f| f.name), Some("texture"));
    assert_eq!(variable("gl_FragCoord").map(|v| v.name), Some("gl_FragCoord"));
    assert!(function("gl_FragCoord").is_none());
    assert!(variable("texture").is_none());
    assert!(function("nonesuch").is_none());
    assert!(is_builtin("mix") && is_builtin("gl_Position"));
    assert!(!is_builtin("myOwnFunction"));
    // Every name in the table is findable through the public lookup.
    for entry in FUNCTIONS {
        assert!(function(entry.name).is_some(), "cannot look up {}", entry.name);
    }
    for entry in VARIABLES {
        assert!(variable(entry.name).is_some(), "cannot look up {}", entry.name);
    }
}

#[test]
fn families_expand_to_concrete_types() {
    let gen_type = family("genType").expect("genType is a family");
    assert_eq!(gen_type.get().members, &["float", "vec2", "vec3", "vec4"]);
    assert!(TypeRef::Family(gen_type).accepts("vec3"));
    assert!(!TypeRef::Family(gen_type).accepts("ivec3"));
    assert!(TypeRef::Concrete("vec4").accepts("vec4"));
    assert_eq!(TypeRef::Concrete("vec4").family(), None);
    // Shadow samplers have no integer flavours, so the `g` family is a single
    // member. research/docs-gl.md §3.1.
    let shadow = family("gsampler2DShadow").expect("gsampler2DShadow is a family");
    assert_eq!(shadow.get().members, &["sampler2DShadow"]);
    assert!(family("vec4").is_none());
}

#[test]
fn doc_slices_are_inside_the_pool_and_on_char_boundaries() {
    let check = |doc: DocRef, what: &str| {
        if doc.is_empty() {
            return;
        }
        // `text()` panics on a bad slice; calling it is the assertion.
        assert!(!doc.text().is_empty(), "{what} has an empty non-empty slice");
    };
    for entry in FUNCTIONS {
        check(entry.doc, entry.name);
        for param in entry.param_docs {
            check(param.doc, param.name);
        }
    }
    for entry in VARIABLES {
        check(entry.doc, entry.name);
    }
}

// ── The hand-written tables (P1-06) ───────────────────────────────────────

#[test]
fn keywords_carry_their_arrival() {
    let buffer = keyword("buffer").expect("buffer is a keyword");
    assert!(!buffer.desktop.contains(DesktopVersion::V420));
    assert!(buffer.desktop.contains(DesktopVersion::V430));
    assert!(buffer.es.contains(EsVersion::V310));
    assert!(!buffer.es.contains(EsVersion::V300));

    // `attribute` left core after 1.30 but lives on in the compatibility
    // profile, which the mask alone cannot say.
    let attribute = keyword("attribute").expect("attribute is a keyword");
    assert!(attribute.desktop.contains(DesktopVersion::V130));
    assert!(!attribute.desktop.contains(DesktopVersion::V140));
    assert!(attribute.compatibility);
    assert!(!keyword("const").expect("const is a keyword").compatibility);

    // `subroutine` and `noperspective` never reached ES.
    assert!(keyword("subroutine").expect("subroutine is a keyword").es.is_empty());
    assert!(keyword("noperspective").expect("noperspective is a keyword").es.is_empty());
}

/// The two lookup orders are built by a `const` insertion sort, and a binary
/// search is only correct over a strictly increasing one. Strictly, not merely
/// weakly: a repeated word would mean the tables disagree with themselves about
/// which entry a name has, and the old linear scan hid that by always answering
/// with the first.
#[test]
fn the_keyword_and_type_orders_are_strictly_sorted() {
    let words: Vec<&str> = KEYWORD_ORDER.iter().map(|&i| KEYWORDS[i as usize].word).collect();
    for pair in words.windows(2) {
        assert!(pair[0] < pair[1], "{:?} is out of order or repeated", pair);
    }
    assert_eq!(words.len(), KEYWORDS.len());

    let names: Vec<&str> = TYPE_ORDER.iter().map(|&i| BASIC_TYPES[i as usize].name).collect();
    for pair in names.windows(2) {
        assert!(pair[0] < pair[1], "{:?} is out of order or repeated", pair);
    }
    assert_eq!(names.len(), BASIC_TYPES.len());

    // Every entry is still reachable by name, which is what the order is for.
    for keyword in KEYWORDS {
        assert_eq!(crate::keyword(keyword.word).map(|k| k.word), Some(keyword.word));
    }
    for ty in BASIC_TYPES {
        assert_eq!(crate::basic_type(ty.name).map(|t| t.name), Some(ty.name));
    }
}

#[test]
fn type_names_are_keywords_but_live_in_their_own_table() {
    assert!(keyword("vec4").is_none());
    assert!(basic_type("vec4").is_some());
    assert!(is_keyword("vec4") && is_keyword("const"));
    assert!(!is_keyword("myVariable"));

    assert_eq!(basic_type("double").expect("double is a type").kind, TypeKind::Scalar);
    assert_eq!(basic_type("mat4x3").expect("mat4x3 is a type").kind, TypeKind::Matrix);
    assert_eq!(basic_type("atomic_uint").expect("atomic_uint").kind, TypeKind::AtomicCounter);
    // ES has no doubles and never had them.
    assert!(basic_type("dvec4").expect("dvec4 is a type").es.is_empty());
    // ES 1.00 predeclares exactly two samplers.
    assert!(basic_type("sampler2D").expect("sampler2D").es.contains(EsVersion::V100));
    assert!(!basic_type("sampler3D").expect("sampler3D").es.contains(EsVersion::V100));

    let names: Vec<&str> = BASIC_TYPES.iter().map(|t| t.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "BASIC_TYPES has a duplicate");
}

#[test]
fn reserved_words_are_not_keywords() {
    assert!(is_reserved("goto"));
    assert!(is_reserved("half"));
    assert!(!is_reserved("float"));
    assert!(!is_reserved("mix"));
    for word in RESERVED_KEYWORDS {
        assert!(!is_keyword(word), "{word} is both reserved and a keyword");
    }
}

#[test]
fn es_precision_defaults_match_the_spec() {
    let es = Version::Es(EsVersion::V300);
    let desktop = Version::Desktop(DesktopVersion::V450);
    // The reason every ES fragment shader opens with `precision mediump float;`.
    assert_eq!(
        default_precision(es, Stage::Fragment, "float"),
        Some(PrecisionDefault::Required)
    );
    assert_eq!(default_precision(es, Stage::Vertex, "float"), Some(PrecisionDefault::High));
    assert_eq!(
        default_precision(es, Stage::Fragment, "int"),
        Some(PrecisionDefault::Medium)
    );
    assert_eq!(
        default_precision(es, Stage::Compute, "sampler2D"),
        Some(PrecisionDefault::Low)
    );
    // Desktop accepts the qualifiers and gives them no meaning, so there is no
    // default to report.
    assert_eq!(default_precision(desktop, Stage::Fragment, "float"), None);
    assert_eq!(default_precision(es, Stage::Vertex, "mat4"), None);
}

// ── Gates on the generated tables (P1-09) ─────────────────────────────────

#[test]
fn mix_has_every_overload_with_the_right_versions() {
    let mix = function("mix").expect("mix is a builtin");
    // Nine prototypes on sl4/mix.xhtml, all structurally distinct, so the ES
    // merge adds none and removes none.
    assert_eq!(mix.overloads.len(), 9, "mix lost or gained an overload");
    assert!(mix.desktop.contains(DesktopVersion::V110));
    assert!(mix.es.contains(EsVersion::V100));

    let count_in = |version| mix.overloads_in(version).count();
    // `mix(genDType…)` arrived in 4.00 and `mix(genIType/genUType/genBType)` in
    // 4.50, so each step adds signatures.
    let v130 = count_in(Version::Desktop(DesktopVersion::V130));
    let v400 = count_in(Version::Desktop(DesktopVersion::V400));
    let v450 = count_in(Version::Desktop(DesktopVersion::V450));
    assert!(v130 < v400, "4.00 should add the double overloads ({v130} → {v400})");
    assert!(v400 < v450, "4.50 should add the integer overloads ({v400} → {v450})");
    assert_eq!(v450, 9);

    // The all-genType signature is the one that has always been there.
    let classic = mix
        .overloads
        .iter()
        .find(|o| o.params.iter().all(|p| p.ty.name() == "genType"))
        .expect("mix(genType, genType, genType)");
    assert!(classic.desktop.contains(DesktopVersion::V110));
    assert!(classic.es.contains(EsVersion::V100));
    assert_eq!(
        classic.signature("mix"),
        "genType mix(genType x, genType y, genType a)"
    );
    assert!(mix.doc.text().contains("interpolation"));
    assert!(mix.param_doc("a").expect("a is documented").text().contains("interpolate"));
}

#[test]
fn texture_keeps_its_gsampler_families_and_both_profiles() {
    let texture = function("texture").expect("texture is a builtin");
    assert!(texture.overloads.len() >= 15, "texture has {} overloads", texture.overloads.len());

    // Desktop since 1.30, ES since 3.00 — and *not* ES 1.00, where the lookup
    // was spelled `texture2D`.
    assert!(texture.desktop.contains(DesktopVersion::V130));
    assert!(texture.es.contains(EsVersion::V300));
    assert!(!texture.es.contains(EsVersion::V100));

    // The signature every shader uses, with its optional bias.
    let gsampler2d = texture
        .overloads
        .iter()
        .find(|o| {
            o.params.first().is_some_and(|p| p.ty.name() == "gsampler2D")
                && o.params.len() == 3
        })
        .expect("texture(gsampler2D, vec2, [float bias])");
    assert_eq!(gsampler2d.ret.name(), "gvec4");
    assert!(gsampler2d.params[2].optional);
    assert_eq!(gsampler2d.arity(), (2, 3));
    assert_eq!(
        gsampler2d.signature("texture"),
        "gvec4 texture(gsampler2D sampler, vec2 P, [float bias])"
    );
    // The family is symbolic data, expandable by overload resolution.
    let family = gsampler2d.params[0].ty.family().expect("gsampler2D is a family");
    assert_eq!(family.members, &["sampler2D", "isampler2D", "usampler2D"]);

    // An ES-only signature: `samplerCubeShadow` takes a `vec4` in ES and a
    // `vec3` on the desktop, so the merge must keep both.
    assert!(
        texture.overloads.iter().any(|o| o.desktop.is_empty() && !o.es.is_empty()),
        "the ES-only texture signatures were merged away"
    );
}

#[test]
fn gl_fragcoord_is_a_fragment_input() {
    let coord = variable("gl_FragCoord").expect("gl_FragCoord is predeclared");
    assert_eq!(coord.ty, "vec4");
    assert_eq!(coord.flow, Flow::In);
    assert_eq!(coord.declaration(), "in vec4 gl_FragCoord");
    assert!(coord.stages.contains(Stage::Fragment));
    assert!(!coord.stages.contains(Stage::Vertex));
    assert!(coord.available_at(Version::Desktop(DesktopVersion::V110), Stage::Fragment));
    assert!(coord.available_at(Version::Es(EsVersion::V100), Stage::Fragment));
    assert!(!coord.available_at(Version::Desktop(DesktopVersion::V450), Stage::Vertex));
    assert!(coord.doc.text().contains("window relative coordinate"));
}

#[test]
fn array_variables_keep_their_suffix() {
    // The `gl_PerVertex` listing page, which has no fieldsynopsis at all.
    let position = variable("gl_Position").expect("gl_Position is predeclared");
    assert_eq!(position.ty, "vec4");
    assert_eq!(position.flow, Flow::Out);
    assert!(position.stages.contains(Stage::Vertex));

    let outer = variable("gl_TessLevelOuter").expect("gl_TessLevelOuter is predeclared");
    assert_eq!(outer.ty, "float[4]");
    assert_eq!(outer.declaration(), "inout float gl_TessLevelOuter[4]");
    assert!(outer.stages.contains(Stage::TessControl));
    assert!(outer.stages.contains(Stage::TessEvaluation));

    let clip = variable("gl_ClipDistance").expect("gl_ClipDistance is predeclared");
    assert_eq!(clip.ty, "float[]");
}

#[test]
fn version_gating_is_real() {
    // `textureGather` is a 4.00 arrival on the desktop.
    let gather = function("textureGather").expect("textureGather is a builtin");
    assert!(gather.available_in(Version::Desktop(DesktopVersion::V400)));
    assert!(!gather.available_in(Version::Desktop(DesktopVersion::V330)));

    // Doubles are desktop-only, so `packDouble2x32` exists nowhere in ES.
    let pack = function("packDouble2x32").expect("packDouble2x32 is a builtin");
    assert!(pack.es.is_empty());

    // And a compute-only variable is not offered to a 1.10 shader.
    let group = variable("gl_WorkGroupSize").expect("gl_WorkGroupSize is predeclared");
    assert!(!group.available_in(Version::Desktop(DesktopVersion::V110)));
    assert!(group.available_in(Version::Desktop(DesktopVersion::V430)));
}

#[test]
fn completion_lists_shrink_with_the_version() {
    let es100 = visible_in(Version::Es(EsVersion::V100)).count();
    let es320 = visible_in(Version::Es(EsVersion::V320)).count();
    let desktop460 = visible_in(Version::Desktop(DesktopVersion::V460)).count();
    assert!(es100 < es320, "ES 1.00 should offer less than ES 3.20 ({es100} vs {es320})");
    assert!(es320 < desktop460, "ES should offer less than desktop 4.60");
    assert!(
        visible_in(Version::Desktop(DesktopVersion::V460))
            .any(|p| p.name() == "gl_FragCoord")
    );
    assert!(!visible_in(Version::Es(EsVersion::V100)).any(|p| p.name() == "textureGather"));
}

#[test]
fn totals_are_in_the_expected_range() {
    // Measured in research/docs-gl.md §1: 161 function names and 31 variables
    // across `sl4/`, with `el3/` a strict subset. A regeneration that moves
    // these more than a little has found a docs.gl change worth reading.
    assert_eq!(COUNTS, (FUNCTIONS.len(), VARIABLES.len(), FAMILIES.len()));
    assert!((150..175).contains(&FUNCTIONS.len()), "{} functions", FUNCTIONS.len());
    assert!((28..40).contains(&VARIABLES.len()), "{} variables", VARIABLES.len());
    assert!((35..50).contains(&FAMILIES.len()), "{} families", FAMILIES.len());
    let overloads: usize = FUNCTIONS.iter().map(|f| f.overloads.len()).sum();
    assert!((650..800).contains(&overloads), "{overloads} overloads");
    // Every function has at least one overload and a non-empty mask.
    for entry in FUNCTIONS {
        assert!(!entry.overloads.is_empty(), "{} has no overloads", entry.name);
        assert!(!entry.availability().is_empty(), "{} exists nowhere", entry.name);
    }
    for entry in VARIABLES {
        assert!(!entry.availability().is_empty(), "{} exists nowhere", entry.name);
        assert!(!entry.stages.is_empty(), "{} has no stage", entry.name);
    }
}

#[test]
fn generated_source_is_within_budget() {
    // RFC 012 §8: the generated Rust source is budgeted at 1.5 MB. Measured
    // from the files themselves so the gate cannot drift from what is
    // committed.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generated");
    let total: u64 = std::fs::read_dir(&dir)
        .expect("generated/ exists")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    assert!(total > 0, "generated/ is empty");
    assert!(
        total <= 1_500_000,
        "generated source is {total} bytes, over the 1.5 MB budget"
    );
    // The doc pool is the part that grows without anyone noticing.
    assert!(DOCS.len() < 400_000, "the doc pool is {} bytes", DOCS.len());
}

#[test]
fn static_footprint_leaves_room_in_the_wasm_budget() {
    // RFC 012 §8 budgets +900 KB of wasm for the whole analyzer. The real
    // number can only be measured once `wgsl-lsp-wasm` links this crate, which
    // is P5-10; what can be measured now is the static data itself — the
    // structs plus the string pool, before the linker dedupes the type-name
    // literals. Treat it as an upper bound on this crate's share.
    use std::mem::size_of_val;
    let mut bytes = DOCS.len();
    bytes += size_of_val(FAMILIES);
    bytes += FAMILIES.iter().map(|f| size_of_val(f.members)).sum::<usize>();
    bytes += size_of_val(FUNCTIONS);
    bytes += size_of_val(VARIABLES);
    for entry in FUNCTIONS {
        bytes += size_of_val(entry.overloads);
        bytes += size_of_val(entry.param_docs);
        bytes += entry.overloads.iter().map(|o| size_of_val(o.params)).sum::<usize>();
    }
    println!("glsl-spec static footprint: {bytes} bytes ({} in the doc pool)", DOCS.len());
    assert!(
        bytes < 600_000,
        "the spec's static data is {bytes} bytes, most of the §8 wasm budget"
    );
}

#[test]
fn the_tables_say_where_they_came_from() {
    assert_eq!(GENERATOR, "glsl-spec-gen");
    assert_eq!(DOCS_GL_COMMIT.len(), 40, "docs.gl commit is not a full hash");
    assert!(DOCS_GL_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
}

// ── The hand-written legacy table (P4-07, decision 0007) ──────────────────

#[test]
fn the_legacy_table_is_well_formed() {
    // Names are the primary key of both tables, and both are read by linear
    // scan — the tables are grouped for review (legacy names, then the builtin
    // constants) the way [`KEYWORDS`] is, so uniqueness is the invariant, not
    // order.
    let functions: Vec<&str> = LEGACY_FUNCTIONS.iter().map(|f| f.function.name).collect();
    let mut unique = functions.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(functions.len(), unique.len(), "LEGACY_FUNCTIONS repeats a name");
    let variables: Vec<&str> = LEGACY_VARIABLES.iter().map(|v| v.variable.name).collect();
    let mut unique = variables.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(variables.len(), unique.len(), "LEGACY_VARIABLES repeats a name");

    for entry in LEGACY_FUNCTIONS {
        assert!(!entry.doc.is_empty(), "{} has no documentation", entry.function.name);
        assert!(
            !entry.function.overloads.is_empty(),
            "{} declares no overload",
            entry.function.name
        );
        // An entry available nowhere and in no profile could never be used.
        assert!(
            !entry.function.availability().is_empty() || entry.compatibility,
            "{} is available nowhere",
            entry.function.name
        );
    }
    for entry in LEGACY_VARIABLES {
        assert!(!entry.doc.is_empty(), "{} has no documentation", entry.variable.name);
        assert!(!entry.variable.stages.is_empty(), "{} exists in no stage", entry.variable.name);
    }
}

#[test]
fn the_legacy_table_does_not_shadow_the_generated_one() {
    // The two tables are consulted one after the other, so an overlap would
    // make which answer wins depend on lookup order. There is no overlap.
    for entry in LEGACY_FUNCTIONS {
        assert!(
            function(entry.function.name).is_none(),
            "{} is in both the generated and the legacy table",
            entry.function.name
        );
    }
    for entry in LEGACY_VARIABLES {
        assert!(
            variable(entry.variable.name).is_none(),
            "{} is in both the generated and the legacy table",
            entry.variable.name
        );
    }
}

#[test]
fn legacy_availability_matches_the_language() {
    // `texture2D` is the WebGL1 spelling and the pre-1.30 desktop spelling,
    // and it is gone from both modern core profiles.
    let texture2d = legacy_function("texture2D").unwrap();
    assert!(texture2d.function.available_in(Version::Es(EsVersion::V100)));
    assert!(texture2d.function.available_in(Version::Desktop(DesktopVersion::V120)));
    assert!(!texture2d.function.available_in(Version::Es(EsVersion::V300)));
    assert!(!texture2d.function.available_in(Version::Desktop(DesktopVersion::V330)));
    // …but the compatibility profile keeps it at every version.
    assert!(texture2d.compatibility);

    // `texture3D` never existed in ES.
    let texture3d = legacy_function("texture3D").unwrap();
    assert!(!texture3d.function.available_in(Version::Es(EsVersion::V100)));

    let frag_color = legacy_variable("gl_FragColor").unwrap();
    assert!(frag_color.variable.available_at(Version::Es(EsVersion::V100), Stage::Fragment));
    assert!(!frag_color.variable.available_at(Version::Es(EsVersion::V100), Stage::Vertex));
    assert!(!frag_color.variable.available_in(Version::Es(EsVersion::V300)));

    // The builtin constants are current, not legacy: they are available in
    // every version, in both profiles, and need no compatibility flag.
    let max_draw_buffers = legacy_variable("gl_MaxDrawBuffers").unwrap();
    assert!(!max_draw_buffers.compatibility);
    for &version in DesktopVersion::ALL {
        assert!(max_draw_buffers.variable.available_in(Version::Desktop(version)));
    }
}
