//! Nesting-depth limit: deeply nested input must not overflow the stack.
//!
//! Every case builds a construct nested `n` levels deep and feeds it through
//! the same `Analyzer` entry points the WASM host calls, on a 1 MB thread
//! (the WASM stack size). Debug-build frames are larger than release/WASM
//! ones, so passing here is the conservative check.

use proto3_analyzer::wasm_api::Analyzer;
use serde_json::Value;

const PROTO_LIMIT: usize = proto3_analyzer::parser::MAX_NESTING_DEPTH;
const TP_LIMIT: usize = proto3_analyzer::textproto::parser::MAX_NESTING_DEPTH;
const HUGE: usize = 100_000;
const STACK: usize = 1 << 20;
const TOO_DEEP: &str = "nesting too deep";

/// One nested construct. Level `i` of `n` is opened by `open(i, n)` on its
/// own line, with its opening token at column `col(i, n)`, and closed by
/// `close(i, n)`. `base` is the depth already used by `prefix`.
#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    prefix: &'static str,
    base: usize,
    open: fn(usize, usize) -> String,
    col: fn(usize, usize) -> u32,
    inner: &'static str,
    close: fn(usize, usize) -> String,
    suffix: &'static str,
}

impl Case {
    /// Source nested `n` levels deep, and the line the after-content is on.
    fn build(&self, n: usize, after: &str) -> (String, u32) {
        let mut s = String::from(self.prefix);
        for i in 0..n {
            s.push_str(&(self.open)(i, n));
            s.push('\n');
        }
        s.push_str(self.inner);
        s.push('\n');
        for i in (0..n).rev() {
            s.push_str(&(self.close)(i, n));
            s.push('\n');
        }
        s.push_str(self.suffix);
        let after_line = s.matches('\n').count() as u32;
        s.push_str(after);
        (s, after_line)
    }

    fn prefix_lines(&self) -> u32 {
        self.prefix.matches('\n').count() as u32
    }
}

fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("bad json ({}): {}", e, &s[..s.len().min(200)]))
}

fn line_of(d: &Value) -> u32 {
    d["start"]["line"].as_u64().unwrap() as u32
}

fn col_of(d: &Value) -> u32 {
    d["start"]["col"].as_u64().unwrap() as u32
}

/// Symbols are checked on the raw JSON: a 64-deep symbol tree is deeper
/// than serde_json's default recursion limit for deserializing (the host's
/// `JSON.parse` has no such limit).
fn has_symbol(symbols_json: &str, name: &str) -> bool {
    symbols_json.contains(&format!("{{\"name\":\"{}\",", name))
}

fn run_on_small_stack<F: FnOnce() + Send + 'static>(name: String, f: F) {
    std::thread::Builder::new()
        .name(name.clone())
        .stack_size(STACK)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap_or_else(|_| panic!("{} panicked", name));
}

/// Assert the too-deep diagnostic (if expected) sits on the opener of the
/// first level past the limit, and that nothing else reports it.
fn check_too_deep(label: &str, case: &Case, limit: usize, n: usize, diags: &Value) {
    let too_deep: Vec<&Value> = diags
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["message"].as_str().unwrap().contains(TOO_DEEP))
        .collect();
    let first_bad = limit - case.base;
    if n <= first_bad {
        assert!(too_deep.is_empty(), "{}: unexpected {:?}", label, too_deep);
        return;
    }
    assert_eq!(too_deep.len(), 1, "{}: {:?}", label, too_deep);
    let d = too_deep[0];
    assert_eq!(d["message"], format!("nesting too deep (limit {})", limit), "{}", label);
    assert_eq!(d["severity"], "error", "{}", label);
    assert_eq!(line_of(d), case.prefix_lines() + first_bad as u32, "{}: {:?}", label, d);
    assert_eq!(col_of(d), (case.col)(first_bad, n), "{}: {:?}", label, d);
}

// ─────────────────────────────── proto3 ───────────────────────────────

const PROTO_HEADER: &str = "syntax = \"proto3\";\n";
const PROTO_AFTER: &str = "message After {\n  Unknown u = 1;\n}\n";

fn message_line(i: usize) -> String {
    format!("message M{} {{", i)
}

fn close_brace(_: usize, _: usize) -> String {
    "}".into()
}

fn col0(_: usize, _: usize) -> u32 {
    0
}

fn proto_cases() -> Vec<Case> {
    vec![
        Case {
            name: "messages",
            prefix: PROTO_HEADER,
            base: 0,
            open: |i, _| message_line(i),
            col: col0,
            inner: "int32 x = 1;",
            close: close_brace,
            suffix: "",
        },
        Case {
            name: "enum in messages",
            prefix: PROTO_HEADER,
            base: 0,
            open: |i, n| if i + 1 == n { "enum E {".into() } else { message_line(i) },
            col: col0,
            inner: "E0 = 0;",
            close: close_brace,
            suffix: "",
        },
        Case {
            name: "oneof in messages",
            prefix: PROTO_HEADER,
            base: 0,
            open: |i, n| if i + 1 == n { "oneof o {".into() } else { message_line(i) },
            col: col0,
            inner: "int32 x = 1;",
            close: close_brace,
            suffix: "",
        },
        Case {
            name: "extend in messages",
            prefix: PROTO_HEADER,
            base: 0,
            open: |i, n| if i + 1 == n { "extend M0 {".into() } else { message_line(i) },
            col: col0,
            inner: "int32 x = 100;",
            close: close_brace,
            suffix: "",
        },
        Case {
            name: "option aggregate",
            prefix: "syntax = \"proto3\";\noption (x) =\n",
            base: 0,
            open: |_, _| "{a:".into(),
            col: col0,
            inner: "1",
            close: close_brace,
            suffix: ";\n",
        },
        Case {
            name: "option list",
            prefix: "syntax = \"proto3\";\noption (x) =\n",
            base: 0,
            open: |_, _| "[".into(),
            col: col0,
            inner: "1",
            close: |_, _| "]".into(),
            suffix: ";\n",
        },
        Case {
            name: "field option aggregate",
            prefix: "syntax = \"proto3\";\nmessage F {\nint32 f = 1 [(x) =\n",
            base: 1,
            open: |_, _| "{a:".into(),
            col: col0,
            inner: "1",
            close: close_brace,
            suffix: "];\n}\n",
        },
        Case {
            name: "map types",
            prefix: "syntax = \"proto3\";\nmessage F {\n",
            base: 1,
            open: |_, _| "map<string,".into(),
            col: col0,
            inner: "int32",
            close: |i, _| if i == 0 { "> f = 1;".into() } else { ">".into() },
            suffix: "}\n",
        },
        Case {
            // Messages and option values share one counter.
            name: "option aggregate in messages",
            prefix: PROTO_HEADER,
            base: 0,
            open: |i, n| match i.cmp(&(n / 2)) {
                std::cmp::Ordering::Less => message_line(i),
                std::cmp::Ordering::Equal => "option (x) = {".into(),
                std::cmp::Ordering::Greater => "a: {".into(),
            },
            col: |i, n| match i.cmp(&(n / 2)) {
                std::cmp::Ordering::Less => 0,
                std::cmp::Ordering::Equal => 13,
                std::cmp::Ordering::Greater => 3,
            },
            inner: "a: 1",
            close: |i, n| if i == n / 2 { "};".into() } else { "}".into() },
            suffix: "",
        },
    ]
}

fn run_proto_case(case: &Case, n: usize) {
    let label = format!("proto {} n={}", case.name, n);
    let (src, after_line) = case.build(n, PROTO_AFTER);
    let deepest = case.prefix_lines() + (n.min(PROTO_LIMIT - case.base) as u32).saturating_sub(1);

    let a = Analyzer::new();
    let uri = "test://deep.proto";
    a.update_file(uri, &src);

    let diags = parse_json(&a.diagnostics(uri));
    check_too_deep(&label, case, PROTO_LIMIT, n, &diags);
    // The declaration after the deep construct still gets diagnostics ...
    assert!(
        diags.as_array().unwrap().iter().any(|d| d["code"] == "PROTO0020" && line_of(d) == after_line + 1),
        "{}: missing unknown-type diagnostic after the deep construct",
        label
    );
    // ... and symbols.
    assert!(has_symbol(&a.document_symbols(uri), "After"), "{}: no `After` symbol", label);

    // Exercise every other entry point over the deep tree.
    a.workspace_symbols("");
    a.folding_ranges(uri);
    a.semantic_tokens(uri);
    a.inlay_hints(uri);
    a.formatting(uri);
    for (line, col) in [(deepest, 0), (deepest, 9), (after_line + 1, 3)] {
        a.hover(uri, line, col);
        a.definition(uri, line, col);
        a.completion(uri, line, col);
        a.references(uri, line, col, true);
        a.prepare_rename(uri, line, col);
        a.rename(uri, line, col, "Renamed");
        a.code_actions(uri, line, col, "[\"PROTO0001\",\"PROTO0020\"]");
    }
    a.remove_file(uri);
}

fn proto_depths(case: &Case) -> [usize; 3] {
    let at_limit = PROTO_LIMIT - case.base;
    [at_limit, at_limit + 1, HUGE]
}

#[test]
fn proto_deep_nesting_is_bounded() {
    for case in proto_cases() {
        for n in proto_depths(&case) {
            let name = format!("proto {} n={}", case.name, n);
            run_on_small_stack(name, move || run_proto_case(&case, n));
        }
    }
}

#[test]
fn proto_real_world_nesting_has_no_depth_error() {
    let src = "syntax = \"proto3\";\n\
               message A { message B { message C { enum E { X = 0; } oneof o { int32 i = 1; } } } }\n\
               option (x) = { a: { b: [1, 2, { c: 3 }] } };\n";
    let a = Analyzer::new();
    a.update_file("test://ok.proto", src);
    let diags = a.diagnostics("test://ok.proto");
    assert!(!diags.contains(TOO_DEEP), "{}", diags);
}

// ────────────────────────────── textproto ──────────────────────────────

const SCHEMA: &str = "syntax = \"proto3\";\npackage t;\n\
                      message A {\n  A a = 1;\n  repeated A l = 2;\n  int32 x = 3;\n  int32 after = 4;\n}\n";
const TP_HEADER: &str = "# proto-file: schema.proto\n# proto-message: t.A\n";
const TP_AFTER: &str = "after: 1\nbogus: 2\n";

fn textproto_cases() -> Vec<Case> {
    vec![
        Case {
            name: "brace messages",
            prefix: TP_HEADER,
            base: 0,
            open: |_, _| "a {".into(),
            col: |_, _| 2,
            inner: "x: 1",
            close: close_brace,
            suffix: "",
        },
        Case {
            name: "angle messages",
            prefix: TP_HEADER,
            base: 0,
            open: |_, _| "a <".into(),
            col: |_, _| 2,
            inner: "x: 1",
            close: |_, _| ">".into(),
            suffix: "",
        },
        Case {
            name: "lists of messages",
            prefix: TP_HEADER,
            base: 0,
            open: |i, _| if i % 2 == 0 { "l: [".into() } else { "{".into() },
            col: |i, _| if i % 2 == 0 { 3 } else { 0 },
            inner: "",
            close: |i, _| if i % 2 == 0 { "]".into() } else { "}".into() },
            suffix: "",
        },
    ]
}

fn run_textproto_case(case: &Case, n: usize) {
    let label = format!("textproto {} n={}", case.name, n);
    let (src, after_line) = case.build(n, TP_AFTER);
    let deepest = case.prefix_lines() + (n.min(TP_LIMIT - case.base) as u32).saturating_sub(1);

    let a = Analyzer::new();
    a.update_file("schema.proto", SCHEMA);
    let uri = "test://deep.textproto";
    a.update_textproto_file(uri, &src);

    let diags = parse_json(&a.textproto_diagnostics(uri));
    check_too_deep(&label, case, TP_LIMIT, n, &diags);
    // The fields after the deep value are still validated against the schema ...
    assert!(
        diags.as_array().unwrap().iter().any(|d| d["code"] == "PROTO0112" && line_of(d) == after_line + 1),
        "{}: missing unknown-field diagnostic after the deep value",
        label
    );
    // ... and get symbols.
    assert!(has_symbol(&a.textproto_document_symbols(uri), "after"), "{}: no `after` symbol", label);

    a.textproto_folding_ranges(uri);
    for (line, col) in [(deepest, 0), (deepest, 3), (after_line, 1)] {
        a.textproto_hover(uri, line, col);
        a.textproto_definition(uri, line, col);
        a.textproto_completion(uri, line, col);
    }
    a.remove_textproto_file(uri);
}

#[test]
fn textproto_deep_nesting_is_bounded() {
    for case in textproto_cases() {
        let at_limit = TP_LIMIT - case.base;
        for n in [at_limit, at_limit + 1, HUGE] {
            let name = format!("textproto {} n={}", case.name, n);
            run_on_small_stack(name, move || run_textproto_case(&case, n));
        }
    }
}
