//! The extensions `#![enable(...)]` can switch on, with the text the
//! hover shows for each.

pub struct Extension {
    pub name: &'static str,
    pub summary: &'static str,
}

pub const EXTENSIONS: &[Extension] = &[
    Extension {
        name: "implicit_some",
        summary: "Values for `Option<T>` fields may be written without `Some(...)`: \
                  `field: 5` instead of `field: Some(5)`.",
    },
    Extension {
        name: "unwrap_newtypes",
        summary: "Newtype structs are written as their inner value: \
                  `meters: 5.0` instead of `meters: Meters(5.0)`.",
    },
    Extension {
        name: "unwrap_variant_newtypes",
        summary: "Newtype enum variants wrapping a struct or tuple drop one set of \
                  parentheses: `Shape(width: 1, height: 2)` instead of \
                  `Shape((width: 1, height: 2))`.",
    },
    Extension {
        name: "explicit_struct_names",
        summary: "Every struct must be written with its name: \
                  `Point(x: 1, y: 2)` rather than `(x: 1, y: 2)`.",
    },
];

pub fn find(name: &str) -> Option<&'static Extension> {
    EXTENSIONS.iter().find(|e| e.name == name)
}
