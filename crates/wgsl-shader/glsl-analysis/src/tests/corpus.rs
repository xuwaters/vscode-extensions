//! P4-10 — the corpus gates: never panic, never cry wolf.
//!
//! Two tests, and they measure opposite things.
//!
//! [`corpus_analyze`] runs the whole glslang `Test/` tree — ~1,700 shaders,
//! most of them deliberately broken — through the analysis under
//! `catch_unwind`, and requires zero panics. Nothing about the diagnostics is
//! asserted: a corpus of deliberate errors has no right answer, and the counts
//! it prints are a number to watch rather than a bar to clear.
//!
//! [`corpus_no_false_errors`] is the bar. It runs a *curated* list of valid
//! shaders — chosen by hand, spanning desktop 1.10 through 4.60, ES 1.00
//! through 3.20, and every stage — and requires **zero error-severity
//! diagnostics**. The list only grows: a file that goes in never comes out,
//! because a rule that needs a file removed from this list is a rule that is
//! wrong.
//!
//! Both read the corpus in place from `temp/glslang/Test` (decision 0004),
//! honour `GLSL_CORPUS`, and skip with a visible message when it is absent.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use analyzer_core::diagnostics::{DiagnosticCode, Severity};
use glsl_spec::Stage;

use crate::{Analysis, Options, analyze_source};

/// The extensions the corpus gate reads, matching the Phase 2 and 3 gates.
const SHADER_EXTENSIONS: &[&str] = &[
    "vert", "frag", "comp", "geom", "tesc", "tese", "glsl", "mesh", "task", "rgen", "rchit",
    "rahit", "rmiss", "rint", "rcall", "vsh", "fsh", "gsh", "glslv", "glslf", "h",
];

fn corpus_root() -> PathBuf {
    if let Ok(path) = std::env::var("GLSL_CORPUS") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").join("temp/glslang/Test")
}

fn shader_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            shader_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SHADER_EXTENSIONS.contains(&e))
        {
            out.push(path);
        }
    }
}

/// Analyse one file the way the server would: the stage from its extension.
fn analyse_file(path: &Path, source: &str) -> Analysis {
    let options = Options::for_path(&path.to_string_lossy());
    analyze_source(source, &options)
}

#[test]
fn corpus_analyze() {
    let root = corpus_root();
    if !root.is_dir() {
        println!(
            "SKIP corpus_analyze: {} is absent. Clone glslang into temp/ to run this gate.",
            root.display()
        );
        return;
    }
    let mut files = Vec::new();
    shader_files(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "{} holds no shaders", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut analysed = 0usize;
    let mut errors = 0usize;
    let mut clean = 0usize;
    let mut histogram: Vec<(&'static str, usize)> = Vec::new();
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        let source = String::from_utf8_lossy(&raw).into_owned();
        analysed += 1;
        match panic::catch_unwind(AssertUnwindSafe(|| {
            let analysis = analyse_file(path, &source);
            let codes: Vec<&'static str> = analysis
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.code.as_str())
                .collect();
            // Every span has to point inside the file the user is looking at.
            for diagnostic in &analysis.diagnostics {
                assert!(
                    diagnostic.span.end as usize <= source.len(),
                    "a diagnostic span runs past the end of the source"
                );
                assert!(
                    source.is_char_boundary(diagnostic.span.start as usize),
                    "a diagnostic span starts mid-character"
                );
            }
            for reference in &analysis.references {
                assert!(
                    reference.span.end as usize <= source.len(),
                    "a reference span runs past the end of the source"
                );
            }
            codes
        })) {
            Ok(codes) => {
                errors += codes.len();
                if codes.is_empty() {
                    clean += 1;
                }
                for code in codes {
                    match histogram.iter_mut().find(|(name, _)| *name == code) {
                        Some((_, count)) => *count += 1,
                        None => histogram.push((code, 1)),
                    }
                }
            }
            Err(_) => failures.push(path.display().to_string()),
        }
    }
    panic::set_hook(previous);

    histogram.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    println!(
        "corpus_analyze: {analysed} files, {} panics, {clean} with no semantic error, \
         {errors} errors total",
        failures.len()
    );
    println!("  by code: {histogram:?}");
    assert!(failures.is_empty(), "these corpus files panicked:\n  {}", failures.join("\n  "));
}

/// The false-positive gate's list: valid shaders that must analyse silently.
///
/// Derived from the corpus itself and then pruned by hand. glslang ships an
/// expected-output file per shader; a shader whose expectation holds no
/// `ERROR:` is one glslang itself considers valid, and that is the list —
/// spanning desktop 1.10 through 4.60, ES 1.00 through 3.20, every stage, and
/// both profiles. Six files it names are excluded, each for the same reason:
/// their validity depends on something outside the file.
///
/// - `glsl.-P.frag`, `glsl.-P.function.frag`, `glsl.-P.include.frag` and
///   `preprocessor.function_macro.vert` are preprocess-only tests: they call
///   functions no file declares, and `vec4(X(3), Y(3,4), Z(3))` really is three
///   components.
/// - `textureQueryLOD.frag` declares its variables inside
///   `#ifdef GL_ARB_texture_query_lod`, a macro only glslang predefines.
/// - `glsl.versionOverride.geom` says `#version 110` and is compiled with a
///   command-line override that makes it something else.
///
/// **This list only grows.** Removing a file to make a rule pass would be
/// deleting the evidence that the rule is wrong.
const VALID: &[&str] = &[
    "100Limits.vert",
    "300link2.frag",
    "300link3.frag",
    "310.inheritMemory.frag",
    "410.vert",
    "460.vert",
    "BestMatchFunction.vert",
    "EndStreamPrimitive.geom",
    "GL_ARB_bindless_texture.frag",
    "GL_ARB_draw_instanced.vert",
    "GL_ARB_fragment_coord_conventions.vert",
    "GL_ARB_gpu_shader5.u2i.vert",
    "GL_ARB_texture_multisample.vert",
    "GL_EXT_draw_instanced.vert",
    "GL_EXT_shader_integer_mix.vert",
    "GL_EXT_texture_array.frag",
    "UTF8BOM.vert",
    "aggOps.frag",
    "always-discard.frag",
    "always-discard2.frag",
    "atomicCounterARBOps.vert",
    "badChars.frag",
    "comment.frag",
    "conditionalDiscard.frag",
    "conservativeDepth.frag",
    "constFoldBitCast64.frag",
    "constantUnaryConversion.comp",
    "conversion.frag",
    "coord_conventions.frag",
    "cppMerge.frag",
    "cppPassMacroName.frag",
    "dataOut.frag",
    "dataOutIndirect.frag",
    "deepRvalue.frag",
    "depthOut.frag",
    "discard-dce.frag",
    "doWhileLoop.frag",
    "earlyReturnDiscard.frag",
    "floatBitsToInt.vert",
    "flowControl.frag",
    "forLoop.frag",
    "forwardRef.frag",
    "functionCall.frag",
    "gl_FragCoord.frag",
    "gl_samplemask_array_size.frag",
    "glsl.-D-U.frag",
    "glsl.450.subgroupArithmetic.comp",
    "glsl.450.subgroupBallot.comp",
    "glsl.450.subgroupClustered.comp",
    "glsl.450.subgroupPartitioned.comp",
    "glsl.450.subgroupQuad.comp",
    "glsl.450.subgroupRotate.comp",
    "glsl.450.subgroupShuffle.comp",
    "glsl.450.subgroupShuffleRelative.comp",
    "glsl.450.subgroupVote.comp",
    "glsl.autosampledtextures.frag",
    "glsl.entryPointRename.vert",
    "glsl.es320.extTextureShadowLod.frag",
    "glsl.es320.subgroup.frag",
    "glsl.es320.subgroup.geom",
    "glsl.es320.subgroup.tesc",
    "glsl.es320.subgroup.tese",
    "glsl.es320.subgroup.vert",
    "glsl.es320.subgroupArithmetic.comp",
    "glsl.es320.subgroupBallot.comp",
    "glsl.es320.subgroupBasic.comp",
    "glsl.es320.subgroupClustered.comp",
    "glsl.es320.subgroupPartitioned.comp",
    "glsl.es320.subgroupQuad.comp",
    "glsl.es320.subgroupRotate.comp",
    "glsl.es320.subgroupShuffle.comp",
    "glsl.es320.subgroupShuffleRelative.comp",
    "glsl.es320.subgroupVote.comp",
    "glsl.nvgpushader5.frag",
    "glsl.nvgpushader5.vert",
    "glsl.versionOverride.comp",
    "glsl.versionOverride.frag",
    "glsl.versionOverride.tesc",
    "glsl.versionOverride.tese",
    "glsl.versionOverride.vert",
    "glspv.esversion.vert",
    "implicitArraySizeBuiltin.vert",
    "implicitArraySizeUniform.vert",
    "include.system.vert",
    "include.vert",
    "iomap.atomicCounter.frag",
    "iomap.bindingPerResourceType.vert",
    "iomap.blockOutVariableIn.2.vert",
    "iomap.blockOutVariableIn.vert",
    "iomap.crossStage.2.vert",
    "iomap.crossStage.vert",
    "iomap.crossStage.vk.vert",
    "iomap.variableOutBlockIn.2.vert",
    "iomap.variableOutBlockIn.vert",
    "length.frag",
    "link.crossStageIO.0.vert",
    "link.crossStageIO.1.vert",
    "link.multiAnonBlocksValid.0.0.vert",
    "link.multiBlocksValid.1.0.vert",
    "link.multiUnitLayout.0.frag",
    "link.multiUnitLayout.0.tese",
    "link.multiUnitLayout.0.vert",
    "link.redeclareBuiltin.vert",
    "link.tesselation.tese",
    "link.tesselation.vert",
    "link.vk.crossStageIO.0.vert",
    "link.vk.crossStageIO.1.vert",
    "link.vk.inconsistentGLPerVertex.0.vert",
    "link.vk.matchingPC.0.0.frag",
    "link.vk.multiBlocksValid.0.0.vert",
    "link.vk.multiBlocksValid.1.0.geom",
    "link.vk.pcNamingValid.0.0.vert",
    "link1.vk.frag",
    "liveTraverser.switch.vert",
    "localAggregates.frag",
    "loops.frag",
    "loopsArtificial.frag",
    "matrix.frag",
    "matrix2.frag",
    "matrixCompMult.vert",
    "maxClipDistances.vert",
    "max_vertices_0.geom",
    "newTexture.frag",
    "nonSquare.vert",
    "overflow_underflow_toinf_0.frag",
    "pointCoord.frag",
    "positive_infinity.frag",
    "precise_struct_block.vert",
    "prepost.frag",
    "preprocess.arb_shading_language_include.vert",
    "preprocess.inactive_stringify.vert",
    "preprocess.include_directive_missing_extension.vert",
    "preprocessor.bad_arg.vert",
    "preprocessor.cpp_style___FILE__.vert",
    "preprocessor.cpp_style_line_directive.vert",
    "preprocessor.defined.vert",
    "preprocessor.edge_cases.vert",
    "preprocessor.elseseen.oob.vert",
    "preprocessor.eof_missing.vert",
    "preprocessor.errors.vert",
    "preprocessor.extensions.vert",
    "preprocessor.include.disabled.vert",
    "preprocessor.include.enabled.vert",
    "preprocessor.line.frag",
    "preprocessor.line.vert",
    "preprocessor.macro.recursion.vert",
    "preprocessor.many.endif.vert",
    "preprocessor.paste_stringify.vert",
    "preprocessor.pragma.vert",
    "preprocessor.shift_out_of_range.vert",
    "preprocessor.simple.vert",
    "preprocessor.string_escaping.frag",
    "preprocessor.stringify_invalid.vert",
    "preprocessor.success_if_parse_would_fail.vert",
    "ps_uint_int.frag",
    "rayQuery-OpConvertUToAccelerationStructureKHR.comp",
    "rayQuery-allOps.comp",
    "rayQuery-allOps.frag",
    "rayQuery-allOps.rgen",
    "rayQuery-global.rgen",
    "rayQuery-initialize.rgen",
    "rayQuery-no-cse.rgen",
    "rayQuery-opacityMicromap.comp",
    "rayQuery-opacityMicromapRayQueryMode.comp",
    "rayQuery-opacityMicromapRayQueryMode.rgen",
    "rayQuery-opacityMicromapRayQueryModeDefault.comp",
    "rayQuery-opacityMicromapRayQueryModeForce2State.comp",
    "rayQuery-opacityMicromapRayQueryModeRead.comp",
    "rayQuery-opacityMicromapRayQueryModeTrue.comp",
    "rayQuery-types.comp",
    "rayQuery.rgen",
    "reflection.frag",
    "reflection.options.geom",
    "reflection.options.vert",
    "reflection.vert",
    "sample.frag",
    "sample.vert",
    "simpleFunctionCall.frag",
    "stringToDouble.vert",
    "structAssignment.frag",
    "structDeref.frag",
    "structure.frag",
    "swizzle.frag",
    "test.frag",
    "texture.frag",
    "types.frag",
    "uniformArray.frag",
    "variableArrayIndex.frag",
    "varyingArray.frag",
    "varyingArrayIndirect.frag",
    "versionsClean.vert",
    "vk.relaxed.changeSet.vert",
    "vk.relaxed.frag",
    "vk.relaxed.link1.frag",
    "vk.relaxed.stagelink.0.0.vert",
    "vk.relaxed.stagelink.vert",
    "voidFunction.frag",
    "vulkan.ast.vert",
    "web.array.frag",
    "web.basic.vert",
    "web.builtins.frag",
    "web.builtins.vert",
    "web.comp",
    "web.controlFlow.frag",
    "web.operations.frag",
    "web.separate.frag",
    "web.texture.frag",
    "whileLoop.frag",
];

#[test]
fn corpus_no_false_errors() {
    let root = corpus_root();
    if !root.is_dir() {
        println!(
            "SKIP corpus_no_false_errors: {} is absent. Clone glslang into temp/ to run \
             this gate.",
            root.display()
        );
        return;
    }
    assert!(
        VALID.len() >= 40,
        "the false-positive gate needs at least 40 files and has {}",
        VALID.len()
    );

    let mut missing: Vec<&str> = Vec::new();
    let mut offenders: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for name in VALID {
        let path = root.join(name);
        let Ok(raw) = std::fs::read(&path) else {
            missing.push(name);
            continue;
        };
        let source = String::from_utf8_lossy(&raw).into_owned();
        checked += 1;
        let analysis = analyse_file(&path, &source);
        for diagnostic in analysis.errors() {
            let line = source[..diagnostic.span.start as usize].lines().count();
            offenders.push(format!(
                "{name}:{line}: {} {}",
                diagnostic.code.as_str(),
                diagnostic.message
            ));
        }
    }
    println!("corpus_no_false_errors: {checked} valid shaders, {} errors", offenders.len());
    // A corpus that moved on is a reason to update the list, not to fail.
    if !missing.is_empty() {
        println!("  (absent from this checkout: {missing:?})");
    }
    assert!(
        offenders.is_empty(),
        "the false-positive gate found errors in valid shaders:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_extensions_examples_analyse_cleanly() {
    // The extension's own examples are the shaders a user opens first, and
    // `examples/test.frag` is the OpenGL-style one RFC 012 §9 promises to
    // validate rather than skip.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extensions/wgsl-shader");
    let examples = root.join("examples");
    if !examples.is_dir() {
        println!("SKIP the_extensions_examples_analyse_cleanly: {} is absent",
            examples.display());
        return;
    }
    let mut files = Vec::new();
    shader_files(&examples, &mut files);
    files.sort();
    let mut offenders: Vec<String> = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let analysis = analyse_file(path, &source);
        for diagnostic in analysis.errors() {
            offenders.push(format!(
                "{}: {} {}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                diagnostic.code.as_str(),
                diagnostic.message
            ));
        }
    }
    println!("the_extensions_examples_analyse_cleanly: {} files", files.len());
    assert!(offenders.is_empty(), "the examples report:\n  {}", offenders.join("\n  "));
}

#[test]
fn the_stage_of_every_corpus_extension_is_either_known_or_honestly_unknown() {
    // A stage the host cannot name must not be guessed *into* a rule, so the
    // ray-tracing and mesh extensions answer `None` rather than something
    // approximate.
    assert_eq!(Options::stage_for_extension("frag"), Some(Stage::Fragment));
    assert_eq!(Options::stage_for_extension("rgen"), None);
    assert_eq!(Options::stage_for_extension("mesh"), None);
    assert_eq!(Options::stage_for_extension("h"), None);
}
