//! Bundled well-known-type `.proto` sources, baked into the binary via
//! `include_str!` so the analyzer works with zero external prerequisites.
//!
//! Each entry is `(relative path under google/protobuf/, source)`.
//! The full set is vendored under `well_known/protos/` by the agent that
//! handles the vendoring task. This module remains valid even if the
//! vendored files are empty — callers treat missing entries as "unknown".

pub fn all() -> &'static [(&'static str, &'static str)] {
    ENTRIES
}

macro_rules! wkt {
    ($name:literal) => {
        (
            concat!("google/protobuf/", $name),
            include_str!(concat!("protos/google/protobuf/", $name)),
        )
    };
}

const ENTRIES: &[(&'static str, &'static str)] = &[
    wkt!("any.proto"),
    wkt!("api.proto"),
    wkt!("descriptor.proto"),
    wkt!("duration.proto"),
    wkt!("empty.proto"),
    wkt!("field_mask.proto"),
    wkt!("source_context.proto"),
    wkt!("struct.proto"),
    wkt!("timestamp.proto"),
    wkt!("type.proto"),
    wkt!("wrappers.proto"),
];
