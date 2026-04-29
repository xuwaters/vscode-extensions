//! Build script — invokes the extractor pipeline and emits
//! `$OUT_DIR/completions.bin`. Re-runs whenever the snapshot or the
//! extractor code changes.

#[path = "src/extractor/mod.rs"]
mod extractor;

use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let snapshot_dir = manifest_dir.join("data").join("fish-snapshot");
    let embed_dir = manifest_dir.join("data").join("embed");
    let blob_path = embed_dir.join("completions.bin");
    std::fs::create_dir_all(&embed_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", embed_dir.display()));

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/extractor/mod.rs");
    println!("cargo:rerun-if-changed=src/extractor/fish_lexer.rs");
    println!("cargo:rerun-if-changed=src/extractor/fish_parser.rs");
    println!("cargo:rerun-if-changed=src/extractor/predicate.rs");
    println!("cargo:rerun-if-changed=src/extractor/snapshot.rs");
    println!("cargo:rerun-if-changed={}", snapshot_dir.display());

    let result = match extractor::run(&snapshot_dir) {
        Ok(r) => r,
        Err(e) => {
            // A missing snapshot dir shouldn't break a fresh checkout —
            // emit an empty blob so downstream crates still link.
            println!(
                "cargo:warning=cli-completions-data-fish: snapshot read failed ({e}); emitting empty blob"
            );
            extractor::BuildResult {
                blob: cli_completions::Builder::new().build(),
                stats: extractor::AggregateStats::default(),
            }
        }
    };

    println!(
        "cargo:warning=cli-completions-data-fish: {} files, {} kept, {} dropped, {} bytes",
        result.stats.files,
        result.stats.kept,
        result.stats.dropped,
        result.blob.len(),
    );

    std::fs::write(&blob_path, &result.blob)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", blob_path.display()));
}
