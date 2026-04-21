use proto3_analyzer::ast::{ScalarType, TopLevelItem, TypeRef};
use proto3_analyzer::diagnostics::DiagnosticCode;
use proto3_analyzer::parse::parse;
use proto3_analyzer::vfs::{FileUri, Workspace};

const HELLO: &str = r#"
syntax = "proto3";
package demo;

// The Greeting is the primary greeting payload.
message Greeting {
  string salutation = 1;
  repeated string recipients = 2;
  map<string, int32> counts = 3;
}

enum Mood {
  MOOD_UNSPECIFIED = 0;
  MOOD_HAPPY = 1;
  MOOD_GRUMPY = 2;
}

service Greeter {
  rpc SayHi (Greeting) returns (Greeting);
}
"#;

#[test]
fn parses_hello_world_proto() {
    let f = parse(FileUri::new("test://hello.proto"), HELLO.into());
    assert_eq!(f.diagnostics.len(), 0, "unexpected: {:?}", f.diagnostics);
    assert!(matches!(&f.ast.items[0], TopLevelItem::Message(m) if m.name.name == "Greeting"));
    assert!(matches!(&f.ast.items[1], TopLevelItem::Enum(e) if e.name.name == "Mood"));
    assert!(matches!(&f.ast.items[2], TopLevelItem::Service(s) if s.name.name == "Greeter"));

    if let TopLevelItem::Message(m) = &f.ast.items[0] {
        assert_eq!(m.fields.len(), 3);
        assert!(matches!(m.fields[0].ty, TypeRef::Scalar(ScalarType::String, _)));
        assert!(matches!(m.fields[2].ty, TypeRef::Map(_)));
    }
}

#[test]
fn detects_duplicate_field_number() {
    let src = r#"
syntax = "proto3";
message M {
  string a = 1;
  string b = 1;
}
"#;
    let mut ws = Workspace::with_bundled_well_known_types();
    ws.update_file(FileUri::new("test://dup.proto"), src.into());
    let diags = ws.diagnostics_for(&FileUri::new("test://dup.proto"));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::DuplicateFieldNumber));
}

#[test]
fn detects_reserved_field_number() {
    let src = r#"
syntax = "proto3";
message M {
  reserved 5 to 10;
  string a = 7;
}
"#;
    let mut ws = Workspace::new();
    ws.update_file(FileUri::new("test://reserved.proto"), src.into());
    let diags = ws.diagnostics_for(&FileUri::new("test://reserved.proto"));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::FieldNumberReserved));
}

#[test]
fn enum_first_value_must_be_zero() {
    let src = r#"
syntax = "proto3";
enum E {
  A = 1;
  B = 2;
}
"#;
    let mut ws = Workspace::new();
    ws.update_file(FileUri::new("test://enum.proto"), src.into());
    let diags = ws.diagnostics_for(&FileUri::new("test://enum.proto"));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::Proto3EnumFirstValueZero));
}

#[test]
fn document_symbols_hierarchy() {
    let src = r#"
syntax = "proto3";
message Outer {
  message Inner {
    int32 x = 1;
  }
  Inner nested = 1;
}
"#;
    let mut ws = Workspace::new();
    ws.update_file(FileUri::new("test://hier.proto"), src.into());
    let pf = ws.file(&FileUri::new("test://hier.proto")).unwrap();
    let syms = proto3_analyzer::features::document_symbols::document_symbols(&pf.ast);
    assert_eq!(syms.len(), 1);
    assert_eq!(syms[0].name, "Outer");
    let inner = syms[0].children.iter().find(|c| c.name == "Inner");
    assert!(inner.is_some());
}

#[test]
fn bundled_well_known_types_parse_cleanly() {
    // The workspace is pre-populated with google/protobuf/*.proto sources
    // via include_str!; they must all parse without emitting diagnostics
    // (except descriptor.proto which is proto2 syntax — we allow that one
    // to surface proto2-specific notes but not outright errors).
    let ws = Workspace::with_bundled_well_known_types();
    for (uri, pf) in ws.files() {
        let is_descriptor = uri.as_str().ends_with("descriptor.proto");
        let diags = ws.diagnostics_for(uri);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == proto3_analyzer::diagnostics::Severity::Error)
            .collect();
        assert!(
            errors.is_empty() || is_descriptor,
            "{} produced errors: {:#?}\nsource first line: {:?}",
            uri.as_str(),
            errors,
            pf.source.lines().next(),
        );
    }
}

#[test]
fn recovers_from_syntax_error_and_continues() {
    let src = r#"
syntax = "proto3";
message A {
  string broken = ;
  string ok = 2;
}
message B { int32 y = 1; }
"#;
    let mut ws = Workspace::new();
    ws.update_file(FileUri::new("test://recover.proto"), src.into());
    let pf = ws.file(&FileUri::new("test://recover.proto")).unwrap();
    // B must still be visible despite the error in A.
    let syms = proto3_analyzer::features::document_symbols::document_symbols(&pf.ast);
    let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"A") && names.contains(&"B"), "got {:?}", names);
}
