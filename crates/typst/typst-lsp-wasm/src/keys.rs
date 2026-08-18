//! How a file's location is described across the WASM boundary.
//!
//! The host needs two things to resolve a read: which root the file hangs off,
//! and the path within it. Rather than marshal a struct on every `World::file`
//! call — which happens hundreds of times during a compile — both go across as
//! plain strings.
//!
//! Compiled for every target, unlike the rest of the crate, so the encoding has
//! unit tests that run under `cargo test` with no WASM toolchain present.

use typst::syntax::VirtualRoot;
use typst::syntax::package::PackageSpec;

/// The root descriptor the host sees.
///
/// * `""` — the project root.
/// * `"@preview/cetz:0.4.2"` — a package, in typst's own spec syntax, so the
///   host can map it onto the cache layout `typst-cli` already uses.
pub fn encode_root(root: &VirtualRoot) -> String {
    match root {
        VirtualRoot::Project => String::new(),
        VirtualRoot::Package(spec) => spec.to_string(),
    }
}

/// The inverse, for the few places the host reports a root back to us.
pub fn decode_root(descriptor: &str) -> Option<VirtualRoot> {
    if descriptor.is_empty() {
        return Some(VirtualRoot::Project);
    }
    descriptor.parse::<PackageSpec>().ok().map(VirtualRoot::Package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_root_is_the_empty_string() {
        assert_eq!(encode_root(&VirtualRoot::Project), "");
        assert_eq!(decode_root(""), Some(VirtualRoot::Project));
    }

    #[test]
    fn a_package_root_round_trips_through_its_spec() {
        let spec: PackageSpec = "@preview/cetz:0.4.2".parse().unwrap();
        let root = VirtualRoot::Package(spec);

        let encoded = encode_root(&root);
        assert_eq!(encoded, "@preview/cetz:0.4.2");
        assert_eq!(decode_root(&encoded), Some(root));
    }

    #[test]
    fn a_malformed_descriptor_decodes_to_nothing() {
        assert_eq!(decode_root("not a spec"), None);
        assert_eq!(decode_root("@preview/cetz"), None);
    }
}
