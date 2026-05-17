//! Integration smoke test against a real diesel-generated `schema.rs`,
//! checked in as a fixture. Confirms the parser handles the full grammar
//! surface diesel currently emits without false-positive diagnostics.

use diesel_schema_analyzer::parse::parse;
use diesel_schema_analyzer::vfs::FileUri;

const SAMPLE: &str = include_str!("fixtures/minichat_schema.rs");

#[test]
fn parses_minichat_schema_cleanly() {
    let pf = parse(FileUri::new("schema.rs"), SAMPLE.to_string());

    // 13 tables, 1 allow group, and 15 joinables in the current sample.
    // If diesel re-emits the file in the future and the counts change,
    // update these assertions — they're a guard against regressions in
    // macro parsing.
    assert_eq!(pf.ast.tables.len(), 13, "expected 13 tables");
    assert_eq!(pf.ast.allow_groups.len(), 1, "expected 1 allow group");
    assert_eq!(pf.ast.joinables.len(), 15, "expected 15 joinables");

    // The real schema should produce zero diagnostics (every table,
    // column, joinable, and allow-group entry resolves).
    let diags: Vec<_> = pf
        .diagnostics
        .iter()
        .map(|d| format!("{:?} {}: {}", d.severity, d.code.as_str(), d.message))
        .collect();
    assert!(diags.is_empty(), "expected no diagnostics, got:\n{}", diags.join("\n"));

    // Spot-check a couple of well-known shapes.
    let users = pf
        .ast
        .tables
        .iter()
        .find(|t| t.name.name == "users")
        .expect("users table");
    let email = users
        .columns
        .iter()
        .find(|c| c.name.name == "email")
        .expect("users.email column");
    assert_eq!(email.sql_type.display, "Citext");

    let messages = pf
        .ast
        .tables
        .iter()
        .find(|t| t.name.name == "messages")
        .expect("messages table");
    let parent_id = messages
        .columns
        .iter()
        .find(|c| c.name.name == "parent_id")
        .expect("messages.parent_id column");
    assert!(parent_id.sql_type.nullable, "parent_id should be Nullable<Text>");
    assert_eq!(parent_id.sql_type.display, "Nullable<Text>");
}
