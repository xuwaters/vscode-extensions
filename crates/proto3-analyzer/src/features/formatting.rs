//! Built-in proto3 pretty-printer.
//!
//! Walks the AST and emits canonical formatting: 2-space indentation,
//! one declaration per line, blank lines between top-level items,
//! leading doc-comments preserved. The formatter is intentionally
//! opinionated and does not try to preserve user horizontal alignment
//! or detached comments.
//!
//! For files with parse diagnostics the formatter refuses and returns
//! `None` — VSCode surfaces that as "no formatting edits" so users see
//! the underlying parse error instead of garbled output.

use crate::ast;
use crate::lexer::{Comment, CommentKind};
use crate::parse::ParsedFile;

const INDENT: &str = "  ";

pub fn format_file(pf: &ParsedFile) -> Option<String> {
    if pf.diagnostics.iter().any(|d| {
        matches!(d.severity, crate::diagnostics::Severity::Error)
    }) {
        return None;
    }
    let mut p = Printer::default();
    p.print_file(&pf.ast);
    Some(p.buf)
}

#[derive(Default)]
struct Printer {
    buf: String,
    depth: usize,
}

impl Printer {
    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.buf.push_str(INDENT);
        }
    }

    fn line(&mut self, s: &str) {
        self.indent();
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    fn blank(&mut self) {
        if !self.buf.ends_with("\n\n") && !self.buf.is_empty() {
            self.buf.push('\n');
        }
    }

    fn print_comments(&mut self, comments: &[Comment]) {
        for c in comments {
            self.indent();
            match c.kind {
                CommentKind::Line => {
                    self.buf.push_str(c.text.as_str());
                    self.buf.push('\n');
                }
                CommentKind::Block => {
                    // Re-emit block comment as-is, preserving internal newlines.
                    let text = c.text.as_str();
                    let mut first = true;
                    for line in text.lines() {
                        if !first {
                            self.buf.push('\n');
                            self.indent();
                        }
                        self.buf.push_str(line);
                        first = false;
                    }
                    self.buf.push('\n');
                }
            }
        }
    }

    fn print_file(&mut self, f: &ast::File) {
        self.print_comments(&f.leading_comments);

        match &f.syntax {
            ast::Syntax::Proto3 => self.line("syntax = \"proto3\";"),
            ast::Syntax::Proto2 => self.line("syntax = \"proto2\";"),
            ast::Syntax::Edition(e) => self.line(&format!("edition = \"{}\";", e)),
            ast::Syntax::Unspecified => {}
        }

        if let Some(pkg) = &f.package {
            self.blank();
            self.line(&format!("package {};", pkg.name.to_display()));
        }

        if !f.imports.is_empty() {
            self.blank();
            let mut imports = f.imports.clone();
            imports.sort_by(|a, b| {
                // Sort by modifier (none, public, weak) then by path.
                use ast::ImportModifier as M;
                fn rank(m: M) -> u8 { match m { M::None => 0, M::Public => 1, M::Weak => 2 } }
                rank(a.modifier)
                    .cmp(&rank(b.modifier))
                    .then_with(|| a.path.cmp(&b.path))
            });
            for imp in &imports {
                let modifier = match imp.modifier {
                    ast::ImportModifier::None => "",
                    ast::ImportModifier::Public => "public ",
                    ast::ImportModifier::Weak => "weak ",
                };
                self.line(&format!("import {}\"{}\";", modifier, imp.path));
            }
        }

        if !f.options.is_empty() {
            self.blank();
            for opt in &f.options {
                self.print_option_decl(opt);
            }
        }

        for item in &f.items {
            self.blank();
            match item {
                ast::TopLevelItem::Message(m) => self.print_message(m),
                ast::TopLevelItem::Enum(e) => self.print_enum(e),
                ast::TopLevelItem::Service(s) => self.print_service(s),
                ast::TopLevelItem::Extend(e) => self.print_extend(e),
            }
        }
    }

    fn print_message(&mut self, m: &ast::Message) {
        self.print_comments(&m.leading_comments);
        self.line(&format!("message {} {{", m.name.name));
        self.depth += 1;
        let mut printed_something = false;
        for opt in &m.options {
            self.print_option_decl(opt);
            printed_something = true;
        }
        for r in &m.reserved {
            self.print_reserved(r);
            printed_something = true;
        }
        for ext in &m.extensions {
            self.print_extensions(ext);
            printed_something = true;
        }
        for f in &m.fields {
            self.print_field(f);
            printed_something = true;
        }
        for o in &m.oneofs {
            if printed_something { self.buf.push('\n'); }
            self.print_oneof(o);
            printed_something = true;
        }
        for nm in &m.nested_messages {
            if printed_something { self.buf.push('\n'); }
            self.print_message(nm);
            printed_something = true;
        }
        for ne in &m.nested_enums {
            if printed_something { self.buf.push('\n'); }
            self.print_enum(ne);
            printed_something = true;
        }
        self.depth -= 1;
        self.line("}");
    }

    fn print_enum(&mut self, e: &ast::EnumDecl) {
        self.print_comments(&e.leading_comments);
        self.line(&format!("enum {} {{", e.name.name));
        self.depth += 1;
        for opt in &e.options {
            self.print_option_decl(opt);
        }
        for r in &e.reserved {
            self.print_reserved(r);
        }
        for v in &e.values {
            self.print_comments(&v.leading_comments);
            let opts = self.render_field_options(&v.options);
            let num = v.number.as_i64().map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            self.line(&format!("{} = {}{};", v.name.name, num, opts));
        }
        self.depth -= 1;
        self.line("}");
    }

    fn print_service(&mut self, s: &ast::Service) {
        self.print_comments(&s.leading_comments);
        self.line(&format!("service {} {{", s.name.name));
        self.depth += 1;
        for opt in &s.options {
            self.print_option_decl(opt);
        }
        for m in &s.methods {
            self.print_comments(&m.leading_comments);
            let input = render_rpc_type(&m.input);
            let output = render_rpc_type(&m.output);
            if m.options.is_empty() {
                self.line(&format!(
                    "rpc {}({}) returns ({});",
                    m.name.name, input, output
                ));
            } else {
                self.line(&format!(
                    "rpc {}({}) returns ({}) {{",
                    m.name.name, input, output
                ));
                self.depth += 1;
                for opt in &m.options {
                    self.print_option_decl(opt);
                }
                self.depth -= 1;
                self.line("}");
            }
        }
        self.depth -= 1;
        self.line("}");
    }

    fn print_extend(&mut self, e: &ast::Extend) {
        self.line(&format!("extend {} {{", e.ty.to_display()));
        self.depth += 1;
        for f in &e.fields {
            self.print_field(f);
        }
        self.depth -= 1;
        self.line("}");
    }

    fn print_field(&mut self, f: &ast::FieldDecl) {
        self.print_comments(&f.leading_comments);
        let label = match f.label {
            ast::FieldLabel::Repeated => "repeated ",
            ast::FieldLabel::Optional => "optional ",
            ast::FieldLabel::Required => "required ",
            ast::FieldLabel::None => "",
        };
        let ty = render_type(&f.ty);
        let num = f.number.as_i64().map(|n| n.to_string()).unwrap_or_else(|| "?".into());
        let opts = self.render_field_options(&f.options);
        self.line(&format!(
            "{}{} {} = {}{};",
            label, ty, f.name.name, num, opts
        ));
    }

    fn print_oneof(&mut self, o: &ast::Oneof) {
        self.line(&format!("oneof {} {{", o.name.name));
        self.depth += 1;
        for opt in &o.options {
            self.print_option_decl(opt);
        }
        for f in &o.fields {
            // Oneof fields have no label.
            let ty = render_type(&f.ty);
            let num = f.number.as_i64().map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            let opts = self.render_field_options(&f.options);
            self.line(&format!("{} {} = {}{};", ty, f.name.name, num, opts));
        }
        self.depth -= 1;
        self.line("}");
    }

    fn print_reserved(&mut self, r: &ast::Reserved) {
        let items: Vec<String> = r
            .items
            .iter()
            .map(|item| match item {
                ast::ReservedItem::Name(n, _) => format!("\"{}\"", n),
                ast::ReservedItem::Number(n) => {
                    let sign = if n.negative { "-" } else { "" };
                    format!("{}{}", sign, n.raw)
                }
                ast::ReservedItem::Range { from, to, .. } => {
                    let lo = format!(
                        "{}{}",
                        if from.negative { "-" } else { "" },
                        from.raw
                    );
                    let hi = match to {
                        ast::ReservedRangeEnd::Value(v) => {
                            format!("{}{}", if v.negative { "-" } else { "" }, v.raw)
                        }
                        ast::ReservedRangeEnd::Max(_) => "max".to_string(),
                    };
                    format!("{} to {}", lo, hi)
                }
            })
            .collect();
        self.line(&format!("reserved {};", items.join(", ")));
    }

    fn print_extensions(&mut self, e: &ast::ExtensionsDecl) {
        let ranges: Vec<String> = e
            .ranges
            .iter()
            .map(|r| {
                let lo = format!("{}{}", if r.from.negative { "-" } else { "" }, r.from.raw);
                match &r.to {
                    ast::ReservedRangeEnd::Value(v) if v.raw == r.from.raw && v.negative == r.from.negative => {
                        lo
                    }
                    ast::ReservedRangeEnd::Value(v) => format!(
                        "{} to {}{}",
                        lo,
                        if v.negative { "-" } else { "" },
                        v.raw
                    ),
                    ast::ReservedRangeEnd::Max(_) => format!("{} to max", lo),
                }
            })
            .collect();
        self.line(&format!("extensions {};", ranges.join(", ")));
    }

    fn print_option_decl(&mut self, opt: &ast::OptionDecl) {
        let name = render_option_name(&opt.name);
        let val = render_option_value(&opt.value, self.depth + 1);
        self.line(&format!("option {} = {};", name, val));
    }

    fn render_field_options(&self, opts: &[ast::OptionDecl]) -> String {
        if opts.is_empty() {
            return String::new();
        }
        let rendered: Vec<String> = opts
            .iter()
            .map(|o| {
                format!(
                    "{} = {}",
                    render_option_name(&o.name),
                    render_option_value(&o.value, self.depth + 1),
                )
            })
            .collect();
        format!(" [{}]", rendered.join(", "))
    }
}

fn render_rpc_type(t: &ast::RpcType) -> String {
    if t.streaming {
        format!("stream {}", t.ty.to_display())
    } else {
        t.ty.to_display()
    }
}

fn render_type(t: &ast::TypeRef) -> String {
    match t {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!("map<{}, {}>", render_type(&m.key), render_type(&m.value)),
        ast::TypeRef::Missing(_) => "?".into(),
    }
}

fn render_option_name(n: &ast::OptionName) -> String {
    let mut s = String::new();
    for (i, part) in n.parts.iter().enumerate() {
        if i > 0 {
            s.push('.');
        }
        if part.is_extension {
            s.push('(');
            s.push_str(&part.name.to_display());
            s.push(')');
        } else {
            s.push_str(&part.name.to_display());
        }
    }
    s
}

fn render_option_value(v: &ast::OptionValue, depth: usize) -> String {
    match v {
        ast::OptionValue::String(s, _) => format!("\"{}\"", escape_string(s)),
        ast::OptionValue::Int(i) => format!(
            "{}{}",
            if i.negative { "-" } else { "" },
            i.raw
        ),
        ast::OptionValue::Float(f, _) => f.to_string(),
        ast::OptionValue::Bool(b, _) => if *b { "true".into() } else { "false".into() },
        ast::OptionValue::Ident(i) => i.name.to_string(),
        ast::OptionValue::Message(fields, _) => render_message_literal(fields, depth),
        ast::OptionValue::List(vs, _) => {
            let inner: Vec<String> = vs.iter().map(|v| render_option_value(v, depth)).collect();
            format!("[{}]", inner.join(", "))
        }
        ast::OptionValue::Missing(_) => "?".into(),
    }
}

fn render_message_literal(fields: &[ast::MessageLiteralField], depth: usize) -> String {
    if fields.is_empty() {
        return "{}".into();
    }
    let indent: String = INDENT.repeat(depth);
    let outer_indent: String = INDENT.repeat(depth.saturating_sub(1));
    let mut out = String::from("{\n");
    for f in fields {
        out.push_str(&indent);
        out.push_str(f.name.name.as_str());
        match &f.value {
            ast::OptionValue::Message(_, _) => {
                out.push(' ');
                out.push_str(&render_option_value(&f.value, depth + 1));
            }
            _ => {
                out.push_str(": ");
                out.push_str(&render_option_value(&f.value, depth + 1));
            }
        }
        out.push('\n');
    }
    out.push_str(&outer_indent);
    out.push('}');
    out
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}
