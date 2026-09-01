//! P2-07 — the glslang corpus gate.
//!
//! `temp/glslang/Test/` holds ~1,700 real shaders across every version, stage
//! and profile, including ones written to be invalid. It is read **in place**
//! and never copied — decision 0004 in `docs/rfc/012-glsl-analyzer/decisions/`
//! — and the test skips with a visible message when the checkout is absent.
//!
//! The bar is **zero panics** and nothing else. Many of these files are
//! supposed to fail to compile, so diagnostics are expected and never compared
//! against glslang's own `.out` files. Alongside the no-panic bar we assert the
//! two invariants that must hold for *any* input: the token stream partitions
//! the source exactly, and every span in the answer points inside it.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use crate::preprocess_source;
use crate::lexer::tokenize;

/// Extensions the extension's `package.json` treats as GLSL, plus the ray-
/// tracing and mesh stages glslang's corpus uses and the `.h` fragments its
/// `#include` tests pull in.
const SHADER_EXTENSIONS: &[&str] = &[
    "vert", "frag", "comp", "geom", "tesc", "tese", "glsl", "mesh", "task", "rgen", "rchit",
    "rahit", "rmiss", "rint", "rcall", "vsh", "fsh", "gsh", "glslv", "glslf", "h",
];

fn corpus_root() -> PathBuf {
    // `GLSL_CORPUS` overrides the location, which is also how the skip path
    // gets exercised without moving the checkout.
    if let Ok(path) = std::env::var("GLSL_CORPUS") {
        return PathBuf::from(path);
    }
    // crates/wgsl-shader/glsl-syntax → the repo root.
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

/// The invariants that must hold for every input, checked per file.
fn check(source: &str) {
    let tokens = tokenize(source);
    let mut at = 0u32;
    for token in &tokens {
        assert_eq!(token.span.start, at, "token stream has a gap or overlap");
        assert!(token.span.end > token.span.start, "empty token");
        at = token.span.end;
    }
    assert_eq!(at as usize, source.len(), "token stream does not reach the end");

    let pp = preprocess_source(source);
    let len = source.len() as u32;
    for token in &pp.tokens {
        assert!(token.span.end <= len, "token span past the end of the source");
        assert!(source.is_char_boundary(token.span.start as usize), "token span mid-scalar");
    }
    for region in &pp.inactive {
        assert!(region.span.end <= len, "inactive region past the end of the source");
    }
    for diagnostic in &pp.diagnostics {
        assert!(diagnostic.span.end <= len, "diagnostic span past the end of the source");
    }
}

#[test]
fn corpus_preprocess() {
    let root = corpus_root();
    if !root.is_dir() {
        println!(
            "SKIP corpus_preprocess: {} is absent. Clone glslang into temp/ to run this gate.",
            root.display()
        );
        return;
    }
    let mut files = Vec::new();
    shader_files(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "{} holds no shaders", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut lexed = 0usize;
    let mut bytes = 0usize;
    // A panic here would abort the whole run and name only the first file, so
    // each is isolated and the report lists every one that broke.
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    for path in &files {
        let Ok(raw) = std::fs::read(path) else {
            continue;
        };
        // Some corpus files are not valid UTF-8; the lexer must cope with
        // whatever an editor would hand it, so lossy conversion is honest here.
        let source = String::from_utf8_lossy(&raw).into_owned();
        bytes += source.len();
        lexed += 1;
        if panic::catch_unwind(AssertUnwindSafe(|| check(&source))).is_err() {
            failures.push(path.display().to_string());
        }
    }
    panic::set_hook(previous);

    println!(
        "corpus_preprocess: {lexed} files, {} KiB, {} panics",
        bytes / 1024,
        failures.len()
    );
    assert!(failures.is_empty(), "these corpus files panicked:\n  {}", failures.join("\n  "));
}
