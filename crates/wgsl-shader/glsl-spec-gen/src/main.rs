//! Generates glsl-spec's tables from a docs.gl checkout at `temp/docs.gl`.
//!
//! RFC 012, Phase 1 (docs/rfc/012-glsl-analyzer/design/spec-pipeline.md).
//!
//! ```text
//! cargo run -p glsl-spec-gen                     # the whole interface
//! cargo run -p glsl-spec-gen -- --check          # fail if the tree is stale
//! cargo run -p glsl-spec-gen -- --docs-gl <dir> --out <dir>
//! ```
//!
//! The checkout is read in place and never copied — it is tens of megabytes of
//! reference pages, and only the tables this writes are committed
//! (`docs/rfc/012-glsl-analyzer/decisions/0002-docs-gl-as-spec-source.md`).
//! Every page that fails to parse stops the run: a silent gap in the builtin
//! table is a wrong answer in an editor, months later, with no way back to the
//! cause.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod collect;
mod emit;
mod markdown;
mod page;
mod prototypes;
mod spec;
mod variables;
mod versions;

#[cfg(test)]
mod tests;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("glsl-spec-gen: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    docs_gl: PathBuf,
    out: PathBuf,
    /// Compare instead of writing — what a determinism test runs.
    check: bool,
}

fn run(args: Vec<String>) -> Result<String, String> {
    let options = parse_args(args)?;
    if !options.docs_gl.join("sl4").is_dir() {
        return Err(format!(
            "no docs.gl checkout at {} — clone github.com/BSVino/docs.gl into temp/docs.gl",
            options.docs_gl.display()
        ));
    }

    let spec = collect::collect(&options.docs_gl)?;
    let commit = commit_of(&options.docs_gl);
    let files = emit::emit(&spec, &commit);

    let mut report = String::new();
    let mut stale = Vec::new();
    let mut bytes = 0usize;
    for file in &files {
        let path = options.out.join(file.name);
        bytes += file.contents.len();
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() == Some(file.contents.as_str()) {
            continue;
        }
        if options.check {
            stale.push(file.name);
        } else {
            std::fs::create_dir_all(&options.out)
                .map_err(|e| format!("cannot create {}: {e}", options.out.display()))?;
            std::fs::write(&path, &file.contents)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
    }
    if !stale.is_empty() {
        return Err(format!(
            "generated/ is stale: {} differ from a fresh run. Run `cargo run -p glsl-spec-gen`.",
            stale.join(", ")
        ));
    }

    let stats = &spec.stats;
    report.push_str(&format!(
        "docs.gl {commit}\n\
         {} pages read, {} redirect stubs skipped\n\
         {} functions, {} variables, {} generic families\n\
         {} prototypes → {} overloads after merging both profiles\n\
         {} of {} overloads matched a version-table row ({}%); the rest inherit \
         their function's mask\n\
         {} bytes of generated Rust in {} files\n",
        stats.pages,
        stats.redirects,
        spec.functions.len(),
        spec.variables.len(),
        spec.families.len(),
        stats.prototypes,
        spec.functions.iter().map(|f| f.overloads.len()).sum::<usize>(),
        stats.matched_overloads,
        stats.total_overloads,
        percent(stats.matched_overloads, stats.total_overloads),
        bytes,
        files.len(),
    ));
    Ok(report)
}

fn percent(part: usize, whole: usize) -> usize {
    (part * 100).checked_div(whole).unwrap_or(0)
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let root = repo_root();
    let mut options = Options {
        docs_gl: root.join("temp/docs.gl"),
        out: root.join("crates/wgsl-shader/glsl-spec/src/generated"),
        check: false,
    };
    let mut rest = args.into_iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--check" => options.check = true,
            "--docs-gl" => {
                options.docs_gl =
                    rest.next().ok_or("--docs-gl needs a directory")?.into();
            }
            "--out" => {
                options.out = rest.next().ok_or("--out needs a directory")?.into();
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(options)
}

/// The workspace root, from this crate's own manifest directory. Two levels up
/// from `crates/wgsl-shader/glsl-spec-gen`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// The commit the pages were read from, for the attribution header. `unknown`
/// when the checkout is not a git repository — a tarball still generates, it
/// just cannot say what it was.
fn commit_of(docs_gl: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(docs_gl)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
