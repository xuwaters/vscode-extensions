//! Generator tests.
//!
//! Two kinds. The pure ones — label parsing, markdown conversion, family
//! expansion — run everywhere against fixtures written here. The rest read
//! `temp/docs.gl` **in place** and skip with a visible message when it is
//! absent, the pattern
//! [decision 0004](../../../../docs/rfc/012-glsl-analyzer/decisions/0004-corpus-in-place.md)
//! sets for the glslang corpus: no reference page is ever copied into this
//! repository.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::collect::{self, Spec};
use crate::emit;
use crate::markdown;
use crate::page::{Profile, Source, load_profile};
use crate::prototypes::{self, Flow};
use crate::spec;
use crate::variables::{self, FRAGMENT, TESS_CONTROL, TESS_EVALUATION, VERTEX};
use crate::versions;

/// The docs.gl checkout, or `None` with a message saying why the test did
/// nothing.
fn docs_gl() -> Option<PathBuf> {
    let dir = crate::repo_root().join("temp/docs.gl");
    if dir.join("sl4").is_dir() {
        return Some(dir);
    }
    eprintln!(
        "skipping: no docs.gl checkout at {} — clone github.com/BSVino/docs.gl there",
        dir.display()
    );
    None
}

/// Parse one page and hand it to `f`. Returns `None` when the checkout is
/// absent, which every caller turns into a skip.
fn with_page<T>(
    profile: Profile,
    stem: &str,
    f: impl FnOnce(&Source, roxmltree::Node<'_, '_>) -> T,
) -> Option<T> {
    let root = docs_gl()?;
    let source = load_profile(&root, profile)
        .expect("the profile directory reads")
        .into_iter()
        .find(|s| s.stem == stem)
        .unwrap_or_else(|| panic!("{profile}/{stem}.xhtml is missing"));
    let document = roxmltree::Document::parse(&source.text).expect("the page is well-formed");
    let root_node = document.root_element();
    Some(f(&source, root_node))
}

/// The whole merged spec, collected once and shared — 314 pages is fast but
/// not free, and half these tests want the same answer.
fn merged() -> Option<&'static Spec> {
    static SPEC: OnceLock<Option<Spec>> = OnceLock::new();
    SPEC.get_or_init(|| {
        let root = docs_gl()?;
        Some(collect::collect(&root).expect("the corpus collects without error"))
    })
    .as_ref()
}

/// Parse a fragment the way a page would be parsed, for the pure tests.
fn fragment<T>(xml: &str, f: impl FnOnce(roxmltree::Node<'_, '_>) -> T) -> T {
    let document = roxmltree::Document::parse(xml).expect("the fixture is well-formed");
    f(document.root_element())
}

// ── Pure: what the generator knows on its own ─────────────────────────────

#[test]
fn errata_normalise_the_type_names_docs_gl_gets_wrong() {
    assert_eq!(spec::normalize_type("gsampler2DDArray"), "gsampler2DArray");
    assert_eq!(spec::normalize_type("gsamplerRect"), "gsampler2DRect");
    assert_eq!(spec::normalize_type("gbufferImage"), "gimageBuffer");
    assert_eq!(spec::normalize_type("gsampler2D"), "gsampler2D");
}

#[test]
fn families_expand_by_rule_not_by_table() {
    assert_eq!(
        spec::family_members("genType"),
        Some(vec!["float".into(), "vec2".into(), "vec3".into(), "vec4".into()])
    );
    assert_eq!(
        spec::family_members("gsampler3D"),
        Some(vec!["sampler3D".into(), "isampler3D".into(), "usampler3D".into()])
    );
    // Shadow samplers have only a float flavour.
    assert_eq!(
        spec::family_members("gsamplerCubeShadow"),
        Some(vec!["samplerCubeShadow".into()])
    );
    assert_eq!(spec::family_members("gvec4"), Some(vec![
        "vec4".into(),
        "ivec4".into(),
        "uvec4".into()
    ]));
    assert_eq!(spec::family_members("vec4"), None);
    assert_eq!(spec::family_members("gl_FragCoord"), None);
}

#[test]
fn comparison_keys_bridge_the_missing_g() {
    // `textureSize`'s version row says `samplerBuffer`; its prototype says
    // `gsamplerBuffer`. research/docs-gl.md §5.1.
    assert_eq!(spec::comparison_key("gsamplerBuffer"), spec::comparison_key("samplerBuffer"));
    assert_eq!(spec::comparison_key("gsamplerRect"), spec::comparison_key("samplerRect"));
    assert_ne!(spec::comparison_key("genType"), spec::comparison_key("enType"));
}

#[test]
fn markdown_keeps_the_shape_and_drops_what_it_cannot_render() {
    let xml = r#"<div class="refsect1" id="description">
        <h2>Description</h2>
        <p><code class="function">f</code> takes <em class="parameter"><code>x</code></em>
           and is <span class="emphasis">special</span>, see
           <a class="citerefentry" href="g"><span class="refentrytitle">g</span></a>.</p>
        <pre class="programlisting">    int a = 1;
    return a;</pre>
        <p>Result is <math><mi>x</mi></math> exactly.</p>
    </div>"#;
    let text = fragment(xml, markdown::render);
    assert!(text.starts_with("`f` takes `x` and is *special*, see `g`."), "{text}");
    assert!(text.contains("```glsl\nint a = 1;\nreturn a;\n```"), "{text}");
    // The MathML became an ellipsis and earned the entry a note.
    assert!(text.contains("Result is … exactly."), "{text}");
    assert!(text.contains("formulas and tables omitted"), "{text}");
    // The section's own heading never makes it into the prose.
    assert!(!text.contains("Description"), "{text}");
}

#[test]
fn markdown_budget_cuts_at_a_sentence() {
    let long = "Sentence one is here. ".repeat(120);
    let xml = format!(r#"<div id="description"><p>{long}</p></div>"#);
    let text = fragment(&xml, markdown::render);
    assert!(text.len() < 1400, "budget not applied: {} bytes", text.len());
    assert!(text.ends_with('…'), "{}", &text[text.len() - 40..]);
    assert!(text.contains("Sentence one is here."));
}

#[test]
fn markdown_drops_a_table_with_a_note() {
    let xml = r#"<div id="description"><p>Before.</p>
        <div class="informaltable"><table><tr><td>x</td></tr></table></div></div>"#;
    let text = fragment(xml, markdown::render);
    assert_eq!(text, "Before.\n\n*(formulas and tables omitted — see the reference page)*");
}

// ── Prototypes (P1-03) ────────────────────────────────────────────────────

#[test]
fn mix_parses_into_nine_overloads() {
    let Some(overloads) =
        with_page(Profile::Desktop, "mix", prototypes::parse)
    else {
        return;
    };
    let overloads = overloads.expect("mix parses");
    assert_eq!(overloads.len(), 9);
    assert!(overloads.iter().all(|o| o.function == "mix"));
    assert_eq!(
        overloads[0].signature(),
        "genType mix(genType x, genType y, genType a)"
    );
    // Every parameter is an ordinary `in` with no brackets.
    assert!(overloads.iter().flat_map(|o| &o.params).all(|p| p.flow == Flow::In && !p.optional));
}

#[test]
fn optional_parameters_and_flow_qualifiers_survive() {
    let Some(texture) =
        with_page(Profile::Desktop, "texture", prototypes::parse)
    else {
        return;
    };
    let texture = texture.expect("texture parses");
    let bias = texture
        .iter()
        .find(|o| o.params.first().is_some_and(|p| p.ty == "gsampler2D"))
        .expect("texture(gsampler2D, …)");
    assert_eq!(bias.params.len(), 3);
    assert!(bias.params[2].optional);
    assert_eq!(bias.params[2].name, "bias");

    let frexp = with_page(Profile::Desktop, "frexp", |source, root| {
        prototypes::parse(source, root).expect("frexp parses")
    })
    .expect("checkout present");
    assert_eq!(frexp[0].params[1].flow, Flow::Out);
}

#[test]
fn a_sole_void_parameter_is_no_parameter() {
    let Some(emit) = with_page(Profile::Desktop, "EmitVertex", |source, root| {
        prototypes::parse(source, root).expect("EmitVertex parses")
    }) else {
        return;
    };
    assert_eq!(emit.len(), 1);
    assert!(emit[0].params.is_empty());
    assert_eq!(emit[0].signature(), "void EmitVertex()");
}

#[test]
fn the_texelfetch_and_texturesize_errata_are_applied() {
    let Some(fetch) = with_page(Profile::Desktop, "texelFetch", |source, root| {
        prototypes::parse(source, root).expect("texelFetch parses")
    }) else {
        return;
    };
    // docs.gl writes `sample sample`; the spec's signature is `int sample`.
    let sample = fetch
        .iter()
        .flat_map(|o| &o.params)
        .find(|p| p.name == "sample")
        .expect("texelFetch has a sample parameter");
    assert_eq!(sample.ty, "int");

    let size = with_page(Profile::Desktop, "textureSize", |source, root| {
        prototypes::parse(source, root).expect("textureSize parses")
    })
    .expect("checkout present");
    let types: Vec<&str> = size.iter().flat_map(|o| &o.params).map(|p| p.ty.as_str()).collect();
    assert!(types.contains(&"gsampler2DRect"), "gsamplerRect was not normalised");
    assert!(!types.contains(&"gsamplerRect"));
}

#[test]
fn a_page_may_declare_several_functions() {
    let Some(pack) = with_page(Profile::Desktop, "packUnorm", |source, root| {
        prototypes::parse(source, root).expect("packUnorm parses")
    }) else {
        return;
    };
    let mut names: Vec<&str> = pack.iter().map(|o| o.function.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names, ["packSnorm2x16", "packSnorm4x8", "packUnorm2x16", "packUnorm4x8"]);
}

// ── Variables (P1-04) ─────────────────────────────────────────────────────

#[test]
fn a_fieldsynopsis_gives_type_flow_and_stage() {
    let Some(variable) = with_page(Profile::Desktop, "gl_FragCoord", |source, root| {
        let rows = versions::parse(source, root).expect("versions parse");
        let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
        variables::parse(source, root, &labels).expect("gl_FragCoord parses")
    }) else {
        return;
    };
    let variable = variable.expect("gl_FragCoord is a variable page");
    assert_eq!(variable.name, "gl_FragCoord");
    assert_eq!(variable.ty, "vec4");
    assert_eq!(variable.flow, Flow::In);
    assert_eq!(variable.stages, FRAGMENT);
}

#[test]
fn two_synopses_merge_into_one_variable() {
    let Some(variable) = with_page(Profile::Desktop, "gl_TessLevelOuter", |source, root| {
        let rows = versions::parse(source, root).expect("versions parse");
        let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
        variables::parse(source, root, &labels).expect("gl_TessLevelOuter parses")
    }) else {
        return;
    };
    let variable = variable.expect("gl_TessLevelOuter is a variable page");
    // `out` in tessellation control and `in` in evaluation.
    assert_eq!(variable.flow, Flow::InOut);
    assert_eq!(variable.ty, "float[4]");
    assert_eq!(variable.stages, TESS_CONTROL | TESS_EVALUATION);
}

#[test]
fn the_gl_pervertex_listing_pages_are_read() {
    let Some(variable) = with_page(Profile::Desktop, "gl_Position", |source, root| {
        let rows = versions::parse(source, root).expect("versions parse");
        let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
        variables::parse(source, root, &labels).expect("gl_Position parses")
    }) else {
        return;
    };
    // No `fieldsynopsis` at all — the type comes out of the block listing.
    let variable = variable.expect("gl_Position is a variable page");
    assert_eq!(variable.ty, "vec4");
    assert_eq!(variable.flow, Flow::Out);
    assert!(variable.stages & VERTEX != 0);
}

// ── Version tables (P1-05, P1-08) ─────────────────────────────────────────

#[test]
fn version_rows_extrapolate_the_columns_docs_gl_lacks() {
    let Some(rows) = with_page(Profile::Desktop, "mix", |source, root| {
        versions::parse(source, root).expect("mix's versions parse")
    }) else {
        return;
    };
    assert_eq!(rows.len(), 3);
    // 4.50 is column 11 and 4.60 has no column; the bit is copied forward.
    for row in &rows {
        assert_eq!(row.bits >> 11 & 1, row.bits >> 12 & 1, "{}", row.label);
    }
    // `mix(genType)` is every version; `mix(genDType)` starts at 4.00.
    let all = rows.iter().find(|r| r.label.contains("genType)")).expect("the genType row");
    assert_eq!(all.bits, 0x1fff);
    let doubles = rows.iter().find(|r| r.label.contains("genDType")).expect("the genDType row");
    assert_eq!(doubles.bits, 0x1fc0);
}

#[test]
fn es_rows_use_the_es_columns() {
    let Some(rows) = with_page(Profile::Es, "texture", |source, root| {
        versions::parse(source, root).expect("texture's ES versions parse")
    }) else {
        return;
    };
    assert_eq!(rows.len(), 1);
    // ES 1.00 no, 3.00 and 3.10 yes, 3.20 extrapolated from 3.10.
    assert_eq!(rows[0].bits, 0b1110);
}

#[test]
fn a_row_label_splits_into_names_and_qualifiers() {
    let Some(rows) = with_page(Profile::Desktop, "textureSize", |source, root| {
        versions::parse(source, root).expect("textureSize's versions parse")
    }) else {
        return;
    };
    let qualified = rows
        .iter()
        .find(|r| r.label.contains("samplerBuffer"))
        .expect("the samplerBuffer row");
    let entry = qualified.entries_for("textureSize").next().expect("a textureSize clause");
    // `samplerBuffer, samplerRect{Shadow}` is three alternatives, not a
    // three-parameter signature.
    assert_eq!(entry.qualifiers.len(), 3);
    assert!(entry.covers("gsamplerBuffer"));
    assert!(entry.covers("gsamplerRect"));
    assert!(!entry.covers("gsampler2D"));

    let catch_all = rows.iter().find(|r| r.label == "textureSize").expect("the plain row");
    assert!(
        catch_all.entries_for("textureSize").next().expect("a clause").is_catch_all()
    );
}

// ── Merging both profiles (P1-05) ─────────────────────────────────────────

#[test]
fn the_merge_gives_one_entry_two_masks() {
    let Some(spec) = merged() else { return };
    let texture = spec
        .functions
        .iter()
        .find(|f| f.name == "texture")
        .expect("texture is in the merged spec");
    assert!(texture.desktop != 0 && texture.es != 0, "texture lost a profile");

    // Doubles are desktop-only; ES never sees `packDouble2x32`.
    let pack = spec
        .functions
        .iter()
        .find(|f| f.name == "packDouble2x32")
        .expect("packDouble2x32 is in the merged spec");
    assert_eq!(pack.es, 0);

    // Signatures shared by both profiles merged rather than doubling up: the
    // corpus has 1,168 prototypes and far fewer distinct signatures.
    let overloads: usize = spec.functions.iter().map(|f| f.overloads.len()).sum();
    assert!(overloads < spec.stats.prototypes, "nothing merged");
    assert_eq!(spec.stats.redirects, 2, "the dFdy stubs should be the only skips");
    assert_eq!(spec.stats.pages, 314);

    // `dFdy` is declared on `dFdx.xhtml`, which is how it survives its stub.
    assert!(spec.functions.iter().any(|f| f.name == "dFdy"));
}

#[test]
fn most_overloads_match_a_version_row() {
    let Some(spec) = merged() else { return };
    let matched = spec.stats.matched_overloads * 100 / spec.stats.total_overloads;
    // The rest inherit their function's union, which is permissive and safe
    // (research/docs-gl.md §5.1). A drop here means docs.gl changed its row
    // labels and the fallback is carrying more than it should.
    assert!(matched >= 85, "only {matched}% of overloads matched a version row");
}

#[test]
fn every_entry_is_sorted_and_documented() {
    let Some(spec) = merged() else { return };
    assert!(spec.functions.windows(2).all(|w| w[0].name < w[1].name));
    assert!(spec.variables.windows(2).all(|w| w[0].name < w[1].name));
    assert!(spec.families.windows(2).all(|w| w[0].0 < w[1].0));
    for function in &spec.functions {
        assert!(!function.doc.is_empty(), "{} has no description", function.name);
        assert!(
            function.overloads.windows(2).all(|w| w[0].raw.signature() < w[1].raw.signature()),
            "{}'s overloads are unsorted",
            function.name
        );
    }
    for variable in &spec.variables {
        assert!(!variable.doc.is_empty(), "{} has no description", variable.name);
    }
}

// ── Emission (P1-08) ──────────────────────────────────────────────────────

#[test]
fn two_runs_produce_identical_bytes() {
    let Some(spec) = merged() else { return };
    let first = emit::emit(spec, "0000000000000000000000000000000000000000");
    let second = emit::emit(spec, "0000000000000000000000000000000000000000");
    assert_eq!(first.len(), second.len());
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.contents, b.contents, "{} differs between runs", a.name);
    }
    // And a second collection of the same checkout agrees with the first.
    let root = docs_gl().expect("checkout present");
    let again = collect::collect(&root).expect("the corpus collects twice");
    let third = emit::emit(&again, "0000000000000000000000000000000000000000");
    for (a, c) in first.iter().zip(&third) {
        assert_eq!(a.contents, c.contents, "{} differs between collections", a.name);
    }
}

#[test]
fn the_committed_tables_match_a_fresh_run() {
    let Some(spec) = merged() else { return };
    let root = docs_gl().expect("checkout present");
    let commit = crate::commit_of(&root);
    let generated = crate::repo_root().join("crates/wgsl-shader/glsl-spec/src/generated");
    for file in emit::emit(spec, &commit) {
        let path = generated.join(file.name);
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        assert_eq!(
            committed,
            file.contents,
            "generated/{} is stale — run `cargo run -p glsl-spec-gen`",
            file.name
        );
    }
}

#[test]
fn the_header_carries_provenance_and_attribution() {
    let Some(spec) = merged() else { return };
    let files = emit::emit(spec, "deadbeef");
    for file in &files {
        assert!(file.contents.starts_with("// Generated by glsl-spec-gen."), "{}", file.name);
        assert!(file.contents.contains("deadbeef"), "{} has no commit", file.name);
        assert!(file.contents.contains("Khronos Group"), "{} has no attribution", file.name);
        assert!(
            file.contents.contains("Open Publication License"),
            "{} has no licence",
            file.name
        );
        assert!(file.contents.ends_with('\n'), "{} has no trailing newline", file.name);
    }
}
