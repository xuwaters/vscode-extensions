//! Validating a RON file against the Rust type it deserializes into.
//!
//! A comment such as `/* @[crate::config::GameConfig] */` before the value
//! names the type ([`annotation`]). The path resolves in the crate whose
//! `Cargo.toml` is nearest above the file ([`crate::rust::world`]), the
//! type is lowered to what RON can express ([`ty`]), and the value is
//! checked against it ([`check`]).

pub mod annotation;
pub mod case;
pub mod check;
pub mod ty;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::rust::parser::parse_type_text;
use crate::rust::syntax::TypeExpr;
use crate::rust::world::{Crate, ItemRef, Res, World};
use crate::workspace::ParsedFile;
use annotation::Annotation;
use check::{Checker, Exts, Facts};
use std::rc::Rc;
use ty::{Lowerer, Ty};

/// Everything the schema learned about one RON file.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub annotation: Option<Annotation>,
    /// The crates the annotation resolves in: the package's library first,
    /// then its binary.
    pub crates: Vec<Rc<Crate>>,
    /// The struct or enum the annotation names, when it names one.
    pub target: Option<ItemRef>,
    /// The type the top-level value must have.
    pub root: Option<Ty>,
    pub exts: Exts,
    pub diagnostics: Vec<Diagnostic>,
    pub facts: Facts,
}

pub fn analyze(pf: &ParsedFile, world: &World) -> Schema {
    let mut schema = Schema { exts: Exts::of(&pf.ast), ..Schema::default() };
    let Some(ann) = annotation::find(&pf.source, &pf.ast) else { return schema };
    schema.annotation = Some(ann.clone());
    let warn = |message: String| Diagnostic::warning(DiagnosticCode::UnresolvedType, message, ann.span);
    if ann.text.is_empty() {
        schema.diagnostics.push(warn("name a Rust type inside `@[...]`, such as `@[crate::config::Config]`".into()));
        return schema;
    }
    schema.crates = world.crates_for(pf.uri.as_str());
    if schema.crates.is_empty() {
        schema
            .diagnostics
            .push(warn(format!("cannot resolve `{}`: no `Cargo.toml` with a `[package]` above this file", ann.text)));
        return schema;
    }
    let Some(te) = parse_type_text(&ann.text) else {
        schema.diagnostics.push(warn(format!("`{}` is not a Rust type", ann.text)));
        return schema;
    };
    let low = Lowerer::new(world);
    let resolved = schema.crates.iter().find_map(|krate| resolve_root(&low, krate.clone(), &te));
    let Some((root, target)) = resolved else {
        let krate = &schema.crates[0];
        schema.diagnostics.push(warn(format!("cannot find `{}` in crate `{}`", ann.text, krate.name)));
        return schema;
    };
    if let Some(value) = &pf.ast.value {
        let mut checker = Checker::new(Lowerer::new(world), &pf.source, schema.exts);
        checker.check(value, &root);
        schema.diagnostics.extend(checker.diags);
        schema.facts = checker.facts;
    }
    schema.root = Some(root);
    schema.target = target;
    schema
}

/// Lower the annotated type at the crate root. `None` when its path does
/// not resolve to something the schema knows.
fn resolve_root(low: &Lowerer, krate: Rc<Crate>, te: &TypeExpr) -> Option<(Ty, Option<ItemRef>)> {
    let scope = low.world.root_scope(krate)?;
    let TypeExpr::Path { global, segments } = te else {
        return Some((low.lower(te, &scope.path, &Vec::new()), None));
    };
    let names: Vec<String> = segments.iter().map(|s| s.name.clone()).collect();
    match low.world.resolve_path(&scope, *global, &names) {
        Some(Res::Item(item)) => {
            let last = segments.last()?;
            let args = last.args.iter().map(|a| low.lower(a, &scope.path, &Vec::new())).collect();
            let ty = low.instantiate(item.clone(), args, 0);
            Some((ty, Some(item)))
        }
        Some(Res::Module(_)) => None,
        // `Vec<crate::Level>`, `HashMap<String, crate::Item>`, `u32`: fine,
        // as long as the outer type is one the schema knows by name.
        Some(Res::External(_)) | None => {
            let ty = low.lower(te, &scope.path, &Vec::new());
            (!matches!(ty, Ty::Any)).then_some((ty, None))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rust::world::tests::world;
    use crate::workspace::{parse, FileUri};

    pub const RON_URI: &str = "file:///game/assets/config.ron";

    pub const LIB: &str = r#"
pub mod config;
"#;

    pub const CONFIG: &str = r#"
use serde::Deserialize;
use std::collections::HashMap;

/// The whole game's settings.
#[derive(Deserialize)]
pub struct GameConfig {
    /// Window size in pixels.
    pub window_size: (u32, u32),
    pub window_title: String,
    #[serde(default)]
    pub fullscreen: bool,
    pub max_fps: u8,
    pub key_bindings: HashMap<String, Action>,
    pub difficulty: Difficulty,
    pub save_slot: Option<u8>,
    pub shapes: Vec<Shape>,
    #[serde(skip)]
    pub runtime: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action { MoveUp, MoveDown, Fire }

#[derive(Deserialize)]
pub struct Difficulty {
    pub start: Level,
    pub adaptive: bool,
}

#[derive(Deserialize)]
pub enum Level { Easy, Hard }

#[derive(Deserialize)]
pub enum Shape {
    Circle(f32),
    Rect { w: f32, h: f32 },
    Point(i32, i32),
}
"#;

    pub fn files() -> Vec<(&'static str, &'static str)> {
        vec![
            ("file:///game/Cargo.toml", "[package]\nname = \"game\"\n"),
            ("file:///game/src/lib.rs", LIB),
            ("file:///game/src/config.rs", CONFIG),
        ]
    }

    pub fn analyzed(src: &str) -> (ParsedFile, Schema, World) {
        let w = world(&files());
        let pf = parse(FileUri::new(RON_URI), src.to_string());
        let schema = analyze(&pf, &w);
        (pf, schema, w)
    }

    fn messages(src: &str) -> Vec<String> {
        let (_, schema, _) = analyzed(src);
        schema.diagnostics.iter().map(|d| format!("{}: {}", analyzer_core::diagnostics::DiagnosticCode::as_str(d.code), d.message)).collect()
    }

    const VALID: &str = r#"/* @[crate::config::GameConfig] */
GameConfig(
    window_size: (800, 600),
    window_title: "PAC-MAN",
    max_fps: 144,
    key_bindings: {"up": move_up, "space": fire},
    difficulty: (start: Easy, adaptive: false),
    save_slot: Some(2),
    shapes: [Circle(1.0), Rect(w: 1, h: 2.5), Point(-1, 2)],
)
"#;

    #[test]
    fn a_valid_file_is_clean() {
        assert_eq!(messages(VALID), Vec::<String>::new());
    }

    #[test]
    fn type_errors() {
        let src = VALID
            .replace("(800, 600)", "(800, 600, 1)")
            .replace("\"PAC-MAN\"", "42")
            .replace("max_fps: 144", "max_fps: 300")
            .replace("move_up", "jump")
            .replace("Easy", "Medium")
            .replace("Some(2)", "2")
            .replace("Circle(1.0)", "Circle")
            .replace("Point(-1, 2)", "Point(w: 1)");
        assert_eq!(
            messages(&src),
            vec![
                "RON017: expected 2 values for `(u32, u32)`, found 3",
                "RON013: expected `String`, found integer",
                "RON019: `300` does not fit in `u8`",
                "RON016: unknown variant `jump` of enum `Action`; expected `move_up`, `move_down`, `fire`",
                "RON016: unknown variant `Medium` of enum `Level`; expected `Easy`, `Hard`",
                "RON013: expected `Option<u8>`, found integer: write `Some(...)` or `None`, or enable `implicit_some`",
                "RON013: `Shape::Circle` needs a value: `Circle(...)`",
                "RON013: expected `Shape::Point` as `(...)` with 2 values",
            ]
        );
    }

    #[test]
    fn fields_missing_unknown_and_misnamed() {
        let src = "// @[crate::config::GameConfig]\nConfig(window_siz: (1, 2), runtime: 3)";
        assert_eq!(
            messages(src),
            vec![
                "RON018: expected `GameConfig`, found `Config`",
                "RON014: unknown field `window_siz` in `GameConfig`; did you mean `window_size`?",
                "RON014: unknown field `runtime` in `GameConfig`",
                "RON015: missing fields `window_size`, `window_title`, `max_fps`, `key_bindings`, `difficulty`, `shapes` in `GameConfig`",
            ]
        );
        let (_, schema, _) = analyzed(src);
        assert_eq!(schema.diagnostics[1].severity, crate::diagnostics::Severity::Warning);
    }

    #[test]
    fn extensions_change_what_is_accepted() {
        let src = VALID.replace("Some(2)", "2").replace("*/\n", "*/\n#![enable(implicit_some)]\n");
        assert_eq!(messages(&src), Vec::<String>::new());
    }

    #[test]
    fn unresolved_annotations_warn() {
        assert_eq!(messages("// @[crate::config::Nope]\n()"), vec!["RON012: cannot find `crate::config::Nope` in crate `game`"]);
        assert_eq!(messages("// @[]\n()").len(), 1);
        let w = world(&[]);
        let pf = parse(FileUri::new("file:///loose/a.ron"), "// @[crate::A]\n()".into());
        let schema = analyze(&pf, &w);
        assert!(schema.diagnostics[0].message.contains("no `Cargo.toml`"), "{:?}", schema.diagnostics);
    }

    #[test]
    fn non_path_annotations() {
        let src = "// @[Vec<crate::config::Level>]\n[Easy, Hard, Nope]";
        assert_eq!(messages(src), vec!["RON016: unknown variant `Nope` of enum `Level`; expected `Easy`, `Hard`"]);
    }

    fn checked(rust: &str, ron: &str) -> Vec<String> {
        let w = world(&[("file:///p/Cargo.toml", "[package]\nname = \"p\"\n"), ("file:///p/src/lib.rs", rust)]);
        let pf = parse(FileUri::new("file:///p/a.ron"), ron.to_string());
        let schema = analyze(&pf, &w);
        schema.diagnostics.iter().map(|d| d.message.clone()).collect()
    }

    #[test]
    fn serde_attributes() {
        let rust = r#"
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct A {
                window_size: u32,
                #[serde(rename = "t", alias = "title")]
                name: String,
                #[serde(flatten)]
                extra: B,
            }
            pub struct B { depth: u8, #[serde(default)] opt: u8 }
        "#;
        assert_eq!(checked(rust, "// @[A]\nA(windowSize: 1, title: \"x\", depth: 2)"), Vec::<String>::new());
        assert_eq!(
            checked(rust, "// @[A]\nA(window_size: 1, t: \"x\")"),
            vec!["unknown field `window_size` in `A`; did you mean `windowSize`?", "missing fields `windowSize`, `depth` in `A`"]
        );
    }

    #[test]
    fn newtypes_transparent_and_from() {
        let rust = r#"
            pub struct Meters(f64);
            #[serde(transparent)] pub struct Id { v: u32 }
            #[serde(from = "String")] pub struct Name { inner: Vec<u8> }
            pub enum Shape { Rect(RectData), Pair((u8, u8)) }
            pub struct RectData { w: u8 }
            pub struct S { m: Meters, id: Id, n: Name, s: Vec<Shape> }
        "#;
        assert_eq!(checked(rust, "// @[S]\n(m: Meters(1.5), id: 3, n: \"a\", s: [Rect((w: 1)), Pair((1, 2))])"), Vec::<String>::new());
        assert_eq!(
            checked(rust, "// @[S]\n#![enable(unwrap_newtypes, unwrap_variant_newtypes)]\n(m: 1.5, id: 3, n: \"a\", s: [Rect(w: 1), Pair(1, 2)])"),
            Vec::<String>::new()
        );
        assert_eq!(checked(rust, "// @[S]\n(m: 1.5, id: 3, n: \"a\", s: [])"), vec![
            "expected newtype struct `Meters`, found float: write `Meters(...)` or enable `unwrap_newtypes`"
        ]);
    }

    #[test]
    fn tagged_and_untagged_enums() {
        let rust = r#"
            #[serde(untagged)] pub enum U { Num(u8), Text(String), Pair { a: u8 } }
            #[serde(tag = "type")] pub enum T { Move { dx: i8 }, Stop }
            #[serde(tag = "t", content = "c")] pub enum Adj { Num(u8), Unit }
            pub struct S { u: Vec<U>, t: Vec<T>, a: Vec<Adj> }
        "#;
        assert_eq!(
            checked(rust, r#"// @[S]
(u: [1, "x", (a: 2)], t: [(type: "Move", dx: -1), (type: "Stop")], a: [(t: "Num", c: 3), (t: "Unit")])"#),
            Vec::<String>::new()
        );
        assert_eq!(
            checked(rust, r#"// @[S]
(u: [true], t: [(type: "Jump"), (dx: 1)], a: [(t: "Num")])"#),
            vec![
                "bool matches no variant of untagged enum `U`",
                "unknown variant `Jump` of enum `T`",
                "missing field `type` naming a variant of `T`",
                "missing field `c` holding `Adj::Num`",
            ]
        );
    }

    #[test]
    fn generics_and_aliases() {
        let rust = r#"
            pub type Grid<T> = Vec<Vec<T>>;
            pub struct Wrap<T> { inner: T, grid: Grid<T> }
        "#;
        assert_eq!(checked(rust, "// @[Wrap<bool>]\n(inner: true, grid: [[false]])"), Vec::<String>::new());
        assert_eq!(checked(rust, "// @[Wrap<bool>]\n(inner: 1, grid: [[2]])"), vec![
            "expected `bool`, found integer",
            "expected `bool`, found integer",
        ]);
    }

    #[test]
    fn facts_for_features() {
        let (_, schema, _) = analyzed(VALID);
        assert!(schema.target.is_some());
        assert!(schema.facts.keys.iter().any(|k| k.expected.name == "window_size"));
        assert!(schema.facts.names.iter().any(|n| n.variant.is_some()));
        assert!(!schema.facts.bodies.is_empty());
    }
}
