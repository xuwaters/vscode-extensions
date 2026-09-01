//! P3-07 — the glslang corpus gate for the parser.
//!
//! Same corpus, same rules as the Phase 2 gate in [`super::corpus`]: read in
//! place from `temp/glslang/Test/` (decision 0004), skipped with a visible
//! message when the checkout is absent, never compared against glslang's own
//! `.out` expectations. Many of these shaders are written to be invalid.
//!
//! The bar is the Phase 3 exit criterion, which is four things per file:
//!
//! 1. **No panic.** Each file runs under `catch_unwind` so one break names
//!    itself instead of aborting the run.
//! 2. **Every token is a leaf, exactly once, in order.** Nothing is dropped.
//! 3. **The tree round-trips the source byte for byte.**
//! 4. **Every span points inside the source**, on a character boundary.
//!
//! The error count is a *snapshot*, printed and recorded in the task file, not
//! asserted: it is the number to watch when the grammar changes, and a corpus
//! full of deliberately broken shaders has no "right" value.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use analyzer_core::diagnostics::Severity;

use crate::cst::{Child, NodeId, SyntaxTree};
use crate::parse_source;

/// Extensions the extension's `package.json` treats as GLSL, plus the ray-
/// tracing and mesh stages glslang's corpus uses and the `.h` fragments its
/// `#include` tests pull in.
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

fn collect_leaves(tree: &SyntaxTree, id: NodeId, out: &mut Vec<u32>) {
    for child in tree.children(id) {
        match *child {
            Child::Token(token) => out.push(token.0),
            Child::Node(node) => collect_leaves(tree, node, out),
        }
    }
}

/// The invariants, checked per file. Returns the error-severity count so the
/// caller can print the snapshot.
fn check(source: &str) -> usize {
    let (pp, tree) = parse_source(source);
    let len = source.len() as u32;

    let mut leaves = Vec::with_capacity(pp.tokens.len());
    collect_leaves(&tree, NodeId::ROOT, &mut leaves);
    assert_eq!(leaves.len(), pp.tokens.len(), "the tree lost or repeated a token");
    for (index, leaf) in leaves.iter().enumerate() {
        assert_eq!(*leaf as usize, index, "the tree holds its tokens out of order");
    }

    let mut at = 0u32;
    for piece in tree.pieces(&pp, source) {
        assert_eq!(piece.span.start, at, "the pieces do not tile the source");
        at = piece.span.end;
    }
    assert_eq!(at, len, "the pieces stop short of the end");
    assert_eq!(tree.reconstruct(&pp, source), source, "the tree does not round-trip");

    for (_, node) in tree.nodes() {
        assert!(node.span.end <= len, "a node span runs past the end of the source");
        assert!(
            source.is_char_boundary(node.span.start as usize),
            "a node span starts mid-scalar"
        );
    }
    for diagnostic in &tree.diagnostics {
        assert!(diagnostic.span.end <= len, "a diagnostic span runs past the end");
    }

    tree.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
}

#[test]
fn corpus_parse() {
    let root = corpus_root();
    if !root.is_dir() {
        println!(
            "SKIP corpus_parse: {} is absent. Clone glslang into temp/ to run this gate.",
            root.display()
        );
        return;
    }
    let mut files = Vec::new();
    shader_files(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "{} holds no shaders", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut parsed = 0usize;
    let mut bytes = 0usize;
    let mut errors = 0usize;
    let mut clean = 0usize;
    let mut glsl = 0usize;
    let mut glsl_clean = 0usize;
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        // Some corpus files are not valid UTF-8; an editor would hand us
        // whatever is in the buffer, so lossy conversion is the honest input.
        let source = String::from_utf8_lossy(&raw).into_owned();
        // glslang keeps its HLSL front-end tests under the same extensions.
        // They are not GLSL and never will parse; they still have to not
        // panic, so they stay in the gate and out of the clean-parse figure.
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_glsl = !name.starts_with("hlsl.") && !name.contains(".hlsl.");
        bytes += source.len();
        parsed += 1;
        glsl += usize::from(is_glsl);
        match panic::catch_unwind(AssertUnwindSafe(|| check(&source))) {
            Ok(count) => {
                errors += count;
                if count == 0 {
                    clean += 1;
                    glsl_clean += usize::from(is_glsl);
                }
            }
            Err(_) => failures.push(path.display().to_string()),
        }
    }
    panic::set_hook(previous);

    println!(
        "corpus_parse: {parsed} files, {} KiB, {} panics, {clean} with no parse error \
         ({glsl_clean} of the {glsl} that are GLSL rather than glslang's HLSL tests), \
         {errors} parse errors total",
        bytes / 1024,
        failures.len()
    );
    assert!(failures.is_empty(), "these corpus files panicked:\n  {}", failures.join("\n  "));
}

