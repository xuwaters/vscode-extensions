//! The §8 performance budget, as a test (P5-10).
//!
//! RFC 012 §8 gives a full reparse and reanalysis of a 1,000-line shader
//! **5 ms native** and **25 ms in wasm**. The server has no incremental
//! parsing — the CST and the analysis are rebuilt on every keystroke — so this
//! is the number that decides whether typing feels alive.
//!
//! **Both targets are met, so both are what this test asserts.** The measured
//! native figure is ~3.4 ms; the per-layer attribution and what closed the gap
//! are in
//! [research/measurements.md](../../../../docs/rfc/012-glsl-analyzer/research/measurements.md).
//! The wasm figure — the one that actually ships — is ~4.9 ms against a 25 ms
//! budget, measured by `src/wasmBudget.test.ts` in the extension.
//!
//! Until the P5-10 performance work this test asserted a *regression ceiling*
//! of 10 ms instead, because the budget was missed and a budget that moves to
//! meet the result is not a budget. There is now no reason to assert anything
//! but §8 itself: 3.4 ms against 5 ms is a third of the budget in hand, which
//! is the slack an unquiet machine needs. The failure message names the layer
//! table, because a regression here is always one layer and never a diffuse
//! slowdown.

/// RFC 012 §8's native target, and what this test fails at.
const BUDGET_MS: f64 = 5.0;

/// Whether the budget is worth asserting at all.
///
/// A debug build is five to ten times slower than the one that ships, so
/// asserting against it would measure the optimiser rather than the analyzer.
/// The measurement still runs and still prints — `cargo test --release` is
/// what enforces it.
const OPTIMISED: bool = !cfg!(debug_assertions);

mod support;

use std::time::Instant;

/// A shader of roughly `lines` lines that exercises every layer: macros and a
/// conditional for the preprocessor, structs and interface blocks for the
/// parser, and enough expressions to make the analyzer work for its answer.
fn shader(lines: usize) -> String {
    let mut source = String::with_capacity(lines * 48);
    source.push_str("#version 450\n#define SCALE(x) ((x) * 2.0)\n#define AMBIENT 0.1\n");
    source.push_str(
        "layout(set = 0, binding = 0) uniform Camera {\n    mat4 view;\n    mat4 proj;\n\
         } camera;\n\
         layout(location = 0) in vec3 v_normal;\n\
         layout(location = 1) in vec2 v_uv;\n\
         layout(location = 0) out vec4 out_colour;\n\
         layout(set = 0, binding = 1) uniform texture2D albedo;\n\
         layout(set = 0, binding = 2) uniform sampler albedo_sampler;\n\
         struct Light {\n    vec3 colour;\n    float intensity;\n};\n",
    );

    let mut written = source.lines().count();
    let mut index = 0;
    while written < lines - 12 {
        source.push_str(&format!(
            "float helper{index}(vec3 normal, float falloff) {{\n\
             \x20   float lambert{index} = max(dot(normalize(normal), vec3(0.0, 1.0, 0.0)), 0.0);\n\
             \x20   vec3 tinted{index} = normal * SCALE(lambert{index}) + vec3(AMBIENT);\n\
             \x20   return clamp(tinted{index}.x / falloff, 0.0, 1.0);\n\
             }}\n"
        ));
        written += 5;
        index += 1;
    }

    source.push_str("#ifdef NEVER\nfloat dead() { return 0.0; }\n#endif\n");
    source.push_str("void main() {\n    float total = AMBIENT;\n");
    for i in 0..index.min(8) {
        source.push_str(&format!("    total += helper{i}(v_normal, 2.0);\n"));
    }
    source.push_str(
        "    vec4 base = texture(sampler2D(albedo, albedo_sampler), v_uv);\n\
         \x20   out_colour = vec4(base.rgb * total, base.a);\n}\n",
    );
    source
}

/// The best of `runs` full builds, in milliseconds.
fn best_of(runs: usize, source: &str) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        let (glsl, parsed) = wgsl_lsp_core::glsl::GlslDocument::build(source, "frag", None);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        // Nothing here may be optimised away.
        assert!(!parsed.symbols.is_empty());
        assert!(glsl.analysis.symbols.len() > 1);
        best = best.min(elapsed);
    }
    best
}

#[test]
fn a_thousand_line_shader_reparses_and_reanalyses_inside_the_native_budget() {
    let source = shader(1_000);
    let lines = source.lines().count();
    assert!((950..1_100).contains(&lines), "the fixture is {lines} lines");

    // One build first, so the measurement is not paying for first-touch page
    // faults on the embedded spec tables.
    let _ = best_of(1, &source);
    let best = best_of(20, &source);

    let build = if OPTIMISED { "release" } else { "debug — not asserted" };
    println!(
        "P5-10 native ({build}): {lines} lines, best of 20 = {best:.2} ms \
         (§8 budget {BUDGET_MS} ms)"
    );
    assert!(
        !OPTIMISED || best <= BUDGET_MS,
        "a full rebuild took {best:.2} ms and RFC 012 §8 gives it {BUDGET_MS} ms; \
         `the_cost_is_reported_per_layer` says which layer grew"
    );
}

/// The six layers separately, so a future regression can be attributed rather
/// than guessed at. This is the table the budget assertion points at.
#[test]
fn the_cost_is_reported_per_layer() {
    let source = shader(1_000);
    let mut best = [f64::MAX; 6];
    for _ in 0..20 {
        let start = Instant::now();
        let raw = glsl_syntax::tokenize(&source);
        best[0] = best[0].min(start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let pp = glsl_syntax::preprocess(
            &source,
            &raw,
            &glsl_syntax::PreprocessOptions::default(),
        );
        best[1] = best[1].min(start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let tree = glsl_syntax::parse(&pp);
        best[2] = best[2].min(start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let analysis = glsl_analysis::analyze(
            &tree,
            &pp,
            &glsl_analysis::Options {
                stage: Some(glsl_spec::Stage::Fragment),
                ..glsl_analysis::Options::default()
            },
        );
        best[3] = best[3].min(start.elapsed().as_secs_f64() * 1000.0);
        assert!(analysis.errors().next().is_none());

        let start = Instant::now();
        let outline = glsl_syntax::outline::outline(&tree, &pp, &source);
        best[4] = best[4].min(start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let parsed =
            wgsl_lsp_core::glsl::adapter::to_parsed(&source, &raw, &outline);
        best[5] = best[5].min(start.elapsed().as_secs_f64() * 1000.0);
        assert!(!parsed.symbols.is_empty());
    }
    println!(
        "P5-10 native by layer: lex {:.2}, preprocess {:.2}, parse {:.2}, analyse {:.2}, \
         outline {:.2}, project {:.2} (ms)",
        best[0], best[1], best[2], best[3], best[4], best[5]
    );
}

/// The same shader through the whole server, which is what an editor actually
/// pays on a keystroke: the document rebuild plus publishing diagnostics.
#[test]
fn an_edit_to_a_thousand_line_shader_is_answered_inside_the_budget() {
    let source = shader(1_000);
    let mut harness = support::Harness::new();
    let uri = harness.open("big.frag", &source);

    let mut best = f64::MAX;
    for i in 0..20 {
        let edited = source.replace("float total = AMBIENT;", &format!("float total = {i}.0;"));
        let start = Instant::now();
        harness.change(&uri, &edited);
        let _ = harness.server().document(&uri).unwrap().parsed().symbols.len();
        best = best.min(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "P5-10 native, through the server: best of 20 = {best:.2} ms \
         (§8 budget {BUDGET_MS} ms)"
    );
    assert!(
        !OPTIMISED || best <= BUDGET_MS,
        "an edit took {best:.2} ms and RFC 012 §8 gives a rebuild {BUDGET_MS} ms; \
         `the_cost_is_reported_per_layer` says which layer grew"
    );
}
