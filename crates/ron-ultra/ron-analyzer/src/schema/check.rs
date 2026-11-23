//! Checking a RON value against a Rust type, the way `ron` + `serde`
//! would deserialize it.
//!
//! Besides diagnostics, the walk leaves [`Facts`] behind: the type expected
//! at each value, the Rust field behind each key, the struct or variant
//! behind each name, and what each container's body should hold. Hover,
//! go-to-definition and completion read those instead of re-deriving
//! types from the cursor.

use super::case;
use super::ty::{Adt, IntKind, Lowerer, Ty};
use crate::ast::{Container, ContainerKind, EntryKey, File, Value, ValueKind};
use crate::diagnostics::{Diagnostic, DiagnosticCode as C};
use crate::rust::syntax::{Field, Fields, ItemKind, Variant};
use crate::rust::world::ItemRef;
use crate::spans::ByteSpan;

/// The `#![enable(...)]` extensions that change what a type accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Exts {
    pub implicit_some: bool,
    pub unwrap_newtypes: bool,
    pub unwrap_variant_newtypes: bool,
    pub explicit_struct_names: bool,
}

impl Exts {
    pub fn of(file: &File) -> Exts {
        let mut e = Exts::default();
        for ext in file.attributes.iter().flat_map(|a| &a.extensions) {
            match ext.name.as_str() {
                "implicit_some" => e.implicit_some = true,
                "unwrap_newtypes" => e.unwrap_newtypes = true,
                "unwrap_variant_newtypes" => e.unwrap_variant_newtypes = true,
                "explicit_struct_names" => e.explicit_struct_names = true,
                _ => {}
            }
        }
        e
    }
}

/// A field a struct body may hold.
#[derive(Debug, Clone)]
pub struct Expected {
    /// The name RON uses, after `rename` / `rename_all`.
    pub name: String,
    pub aliases: Vec<String>,
    pub ty: Ty,
    pub required: bool,
    /// The struct or enum declaring the field.
    pub owner: ItemRef,
    /// The variant, for a struct variant's field.
    pub variant: Option<usize>,
    pub index: usize,
}

impl Expected {
    pub fn field(&self) -> &Field {
        field_of(&self.owner, self.variant, self.index)
    }
}

pub fn field_of(owner: &ItemRef, variant: Option<usize>, index: usize) -> &Field {
    let fields = match (&owner.item().kind, variant) {
        (ItemKind::Struct { fields, .. }, None) => fields,
        (ItemKind::Enum { variants, .. }, Some(v)) => &variants[v].fields,
        _ => unreachable!("field owners are structs or enum variants"),
    };
    &fields.list()[index]
}

pub fn variant_of(owner: &ItemRef, index: usize) -> Option<&Variant> {
    match &owner.item().kind {
        ItemKind::Enum { variants, .. } => variants.get(index),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct FieldSet {
    pub fields: Vec<Expected>,
    /// Unknown fields are accepted (a flattened map or opaque type) and
    /// their values checked against this.
    pub open: Option<Ty>,
    pub deny_unknown: bool,
    /// `GameConfig`, `Shape::Rect`: for messages.
    pub owner_name: String,
}

/// What a container's body should hold.
#[derive(Debug, Clone)]
pub enum Body {
    Fields(FieldSet),
    Positional(Vec<Ty>),
    Elements(Ty),
    Map(Ty, Ty),
}

#[derive(Debug, Clone)]
pub struct BodyFact {
    /// From the opening bracket to the closing one (or the end of file).
    pub span: ByteSpan,
    /// The closing bracket is there.
    pub closed: bool,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub struct KeyFact {
    pub span: ByteSpan,
    pub expected: Expected,
}

#[derive(Debug, Clone)]
pub struct NameFact {
    pub span: ByteSpan,
    pub owner: ItemRef,
    pub variant: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub values: Vec<(ByteSpan, Ty)>,
    pub keys: Vec<KeyFact>,
    pub names: Vec<NameFact>,
    pub bodies: Vec<BodyFact>,
}

pub struct Checker<'w, 's> {
    low: Lowerer<'w>,
    src: &'s str,
    ext: Exts,
    /// Trial mode, for untagged enums: count errors, record nothing.
    quiet: bool,
    errors: usize,
    pub diags: Vec<Diagnostic>,
    pub facts: Facts,
}

impl<'w, 's> Checker<'w, 's> {
    pub fn new(low: Lowerer<'w>, src: &'s str, ext: Exts) -> Self {
        Checker { low, src, ext, quiet: false, errors: 0, diags: Vec::new(), facts: Facts::default() }
    }

    fn error(&mut self, code: C, message: impl Into<String>, span: ByteSpan) {
        self.errors += 1;
        if !self.quiet {
            self.diags.push(Diagnostic::error(code, message, span));
        }
    }

    fn warn(&mut self, code: C, message: impl Into<String>, span: ByteSpan) {
        if !self.quiet {
            self.diags.push(Diagnostic::warning(code, message, span));
        }
    }

    fn record_body(&mut self, c: &Container, body: Body) {
        if !self.quiet {
            let end = c.close.map_or(self.src.len() as u32, |s| s.end);
            self.facts.bodies.push(BodyFact { span: ByteSpan::new(c.open.start, end), closed: c.close.is_some(), body });
        }
    }

    fn record_name(&mut self, span: ByteSpan, owner: &ItemRef, variant: Option<usize>) {
        if !self.quiet {
            self.facts.names.push(NameFact { span, owner: owner.clone(), variant });
        }
    }

    /// How many errors checking `f` would produce, without reporting them.
    fn trial(&self, f: impl FnOnce(&mut Checker<'w, 's>)) -> usize {
        let mut sub = Checker::new(Lowerer::new(self.low.world), self.src, self.ext);
        sub.quiet = true;
        f(&mut sub);
        sub.errors
    }

    fn mismatch(&mut self, v: &Value, ty: &Ty) {
        let message = format!("expected {}, found {}", label(ty), v.describe());
        self.error(C::TypeMismatch, message, head(v));
    }

    pub fn check(&mut self, v: &Value, ty: &Ty) {
        if !self.quiet {
            self.facts.values.push((v.span, ty.clone()));
        }
        if matches!(v.kind, ValueKind::Missing) {
            return;
        }
        match ty {
            Ty::Any => {}
            Ty::Unit => {
                if !is_unit(v) {
                    self.mismatch(v, ty);
                }
            }
            Ty::Bool => {
                if !matches!(v.kind, ValueKind::Bool(_)) {
                    self.mismatch(v, ty);
                }
            }
            Ty::Int(k) => match v.kind {
                ValueKind::Integer => self.int_range(v, *k),
                _ => self.mismatch(v, ty),
            },
            Ty::Float(_) => {
                if !matches!(v.kind, ValueKind::Float | ValueKind::Integer) {
                    self.mismatch(v, ty);
                }
            }
            Ty::Char => {
                if !matches!(v.kind, ValueKind::Char) {
                    self.mismatch(v, ty);
                }
            }
            Ty::Str => {
                if !matches!(v.kind, ValueKind::String { .. }) {
                    self.mismatch(v, ty);
                }
            }
            Ty::Option(inner) => self.option(v, inner, ty),
            Ty::Seq(_, el) => match &v.kind {
                ValueKind::Container(c) if c.kind == ContainerKind::List => {
                    self.record_body(c, Body::Elements((**el).clone()));
                    for e in &c.entries {
                        self.check(&e.value, el);
                    }
                }
                ValueKind::ByteString if matches!(**el, Ty::Int(IntKind::U8)) => {}
                _ => self.mismatch(v, ty),
            },
            Ty::Array(el, n) => match &v.kind {
                ValueKind::Container(c) if matches!(c.kind, ContainerKind::List | ContainerKind::Tuple) => {
                    self.record_body(c, Body::Elements((**el).clone()));
                    if let Some(n) = *n {
                        if c.entries.len() as u64 != n {
                            let message = format!("expected {} for `{ty}`, found {}", count(n as usize, "value"), c.entries.len());
                            self.error(C::WrongLength, message, c.open);
                        }
                    }
                    for e in &c.entries {
                        self.check(&e.value, el);
                    }
                }
                _ => self.mismatch(v, ty),
            },
            Ty::Tuple(ts) => match &v.kind {
                ValueKind::Container(c) if c.kind == ContainerKind::Tuple => {
                    self.positional(c, ts, &format!("`{ty}`"));
                }
                _ => self.mismatch(v, ty),
            },
            Ty::Map(_, k, val) => match &v.kind {
                ValueKind::Container(c) if c.kind == ContainerKind::Map => {
                    self.record_body(c, Body::Map((**k).clone(), (**val).clone()));
                    for e in &c.entries {
                        if let Some(EntryKey::Map(key)) = &e.key {
                            self.check(key, k);
                        }
                        self.check(&e.value, val);
                    }
                }
                _ => self.mismatch(v, ty),
            },
            Ty::Adt(adt) => self.adt(v, adt, ty),
        }
    }

    fn int_range(&mut self, v: &Value, k: IntKind) {
        let raw = v.raw(self.src);
        if let Some((negative, magnitude)) = parse_ron_int(raw) {
            if !k.fits(negative, magnitude) {
                self.error(C::OutOfRange, format!("`{raw}` does not fit in `{}`", k.name()), v.span);
            }
        }
    }

    fn option(&mut self, v: &Value, inner: &Ty, ty: &Ty) {
        match &v.kind {
            ValueKind::Named { name, body: None } if name.name == "None" => {}
            ValueKind::Named { name, body: Some(c) } if name.name == "Some" => {
                if c.kind == ContainerKind::Tuple && c.entries.len() == 1 {
                    self.record_body(c, Body::Positional(vec![inner.clone()]));
                    self.check(&c.entries[0].value, inner);
                } else {
                    self.error(C::WrongLength, "`Some` takes exactly one value", c.open);
                }
            }
            _ if self.ext.implicit_some => self.check(v, inner),
            _ => {
                let message = format!(
                    "expected `{ty}`, found {}: write `Some(...)` or `None`, or enable `implicit_some`",
                    v.describe()
                );
                self.error(C::TypeMismatch, message, head(v));
            }
        }
    }

    /// `(a, b)`-style bodies: tuples, tuple structs, tuple variants.
    fn positional(&mut self, c: &Container, tys: &[Ty], what: &str) {
        self.record_body(c, Body::Positional(tys.to_vec()));
        if c.kind != ContainerKind::Tuple {
            let message = format!("expected {what} as `(...)` with {}", count(tys.len(), "value"));
            self.error(C::TypeMismatch, message, c.open);
            return;
        }
        if c.entries.len() != tys.len() {
            let message = format!("expected {} for {what}, found {}", count(tys.len(), "value"), c.entries.len());
            self.error(C::WrongLength, message, c.open);
        }
        for (e, t) in c.entries.iter().zip(tys) {
            self.check(&e.value, t);
        }
    }

    /// Whether `v` is spelled `Name(...)`, `Name`, or `(...)`, after checking
    /// the name against `expected`.
    fn form<'v>(&mut self, v: &'v Value, expected: &str, owner: &ItemRef) -> Form<'v> {
        match &v.kind {
            ValueKind::Named { name, body } => {
                if name.name != expected {
                    let message = format!("expected `{expected}`, found `{}`", name.name);
                    self.error(C::WrongName, message, name.span);
                } else {
                    self.record_name(name.span, owner, None);
                }
                body.as_ref().map_or(Form::Bare, Form::Body)
            }
            ValueKind::Container(c) => {
                if self.ext.explicit_struct_names && c.kind != ContainerKind::List && c.kind != ContainerKind::Map {
                    let message = format!("`explicit_struct_names` is enabled: write `{expected}(...)`");
                    self.error(C::WrongName, message, c.open);
                }
                Form::Body(c)
            }
            _ => Form::Wrong,
        }
    }

    fn adt(&mut self, v: &Value, adt: &Adt, ty: &Ty) {
        if let Some(from) = self.low.from_ty(adt) {
            return self.check(v, &from);
        }
        let item = adt.item.item();
        match &item.kind {
            ItemKind::Struct { fields, .. } => {
                if item.serde.transparent {
                    if let Some(f) = fields.list().iter().find(|f| !f.serde.skip) {
                        let inner = self.low.field_ty(adt, f);
                        self.check(v, &inner);
                    }
                    return;
                }
                let name = item.serde.rename.clone().unwrap_or_else(|| item.name().to_string());
                match fields {
                    Fields::Named(fs) => match self.form(v, &name, &adt.item) {
                        Form::Body(c) => {
                            let set = field_set(
                                &self.low,
                                adt,
                                None,
                                fs,
                                item.serde.rename_all.as_deref(),
                                item.serde.default,
                                item.serde.deny_unknown_fields,
                                &name,
                            );
                            self.named_fields(c, set, head(v), None);
                        }
                        Form::Bare => {
                            self.error(C::TypeMismatch, format!("struct `{name}` needs its fields: `{name}(...)`"), head(v));
                        }
                        Form::Wrong => self.mismatch(v, ty),
                    },
                    Fields::Tuple(fs) if fs.len() == 1 => {
                        let inner = self.low.field_ty(adt, &fs[0]);
                        if self.ext.unwrap_newtypes {
                            return self.check(v, &inner);
                        }
                        match self.form(v, &name, &adt.item) {
                            Form::Body(c) => self.positional(c, &[inner], &format!("`{name}`")),
                            Form::Bare | Form::Wrong => {
                                let message = format!(
                                    "expected newtype struct `{name}`, found {}: write `{name}(...)` or enable `unwrap_newtypes`",
                                    v.describe()
                                );
                                self.error(C::TypeMismatch, message, head(v));
                            }
                        }
                    }
                    Fields::Tuple(fs) => {
                        let tys: Vec<Ty> = fs.iter().map(|f| self.low.field_ty(adt, f)).collect();
                        match self.form(v, &name, &adt.item) {
                            Form::Body(c) => self.positional(c, &tys, &format!("`{name}`")),
                            Form::Bare if tys.is_empty() => {}
                            Form::Bare | Form::Wrong => self.mismatch(v, ty),
                        }
                    }
                    Fields::Unit => match self.form(v, &name, &adt.item) {
                        Form::Bare => {}
                        Form::Body(c) if c.kind == ContainerKind::Tuple && c.entries.is_empty() => {}
                        Form::Body(c) => {
                            self.error(C::TypeMismatch, format!("unit struct `{name}` takes no value"), c.open);
                        }
                        Form::Wrong => self.mismatch(v, ty),
                    },
                }
            }
            ItemKind::Enum { variants, .. } => self.enumeration(v, adt, variants, ty),
            _ => {}
        }
    }

    /// A struct-like body. `skip` is a field to ignore, the tag of an
    /// internally tagged enum.
    fn named_fields(&mut self, c: &Container, set: FieldSet, err_span: ByteSpan, skip: Option<&str>) {
        self.record_body(c, Body::Fields(set.clone()));
        match c.kind {
            ContainerKind::Struct => {}
            ContainerKind::Tuple if c.entries.is_empty() => {}
            _ => {
                let message = format!("expected named fields for `{}`, as in `(field: value)`", set.owner_name);
                self.error(C::TypeMismatch, message, c.open);
                return;
            }
        }
        let mut seen = vec![false; set.fields.len()];
        for e in &c.entries {
            let Some(EntryKey::Field(id)) = &e.key else { continue };
            if skip == Some(id.name.as_str()) {
                continue;
            }
            if let Some(i) = set.fields.iter().position(|f| f.name == id.name || f.aliases.contains(&id.name)) {
                seen[i] = true;
                let expected = set.fields[i].clone();
                if !self.quiet {
                    self.facts.keys.push(KeyFact { span: id.span, expected: expected.clone() });
                }
                self.check(&e.value, &expected.ty);
            } else if let Some(open) = &set.open {
                let open = open.clone();
                self.check(&e.value, &open);
            } else {
                let hint = case::suggest(&id.name, set.fields.iter().map(|f| f.name.as_str()))
                    .map(|s| format!("; did you mean `{s}`?"))
                    .unwrap_or_default();
                let message = format!("unknown field `{}` in `{}`{hint}", id.name, set.owner_name);
                if set.deny_unknown {
                    self.error(C::UnknownField, message, id.span);
                } else {
                    self.warn(C::UnknownField, message, id.span);
                }
            }
        }
        let missing: Vec<String> = set
            .fields
            .iter()
            .zip(&seen)
            .filter(|(f, seen)| f.required && !**seen)
            .map(|(f, _)| format!("`{}`", f.name))
            .collect();
        if !missing.is_empty() {
            let noun = if missing.len() == 1 { "field" } else { "fields" };
            let message = format!("missing {noun} {} in `{}`", missing.join(", "), set.owner_name);
            self.error(C::MissingField, message, err_span);
        }
    }

    fn enumeration(&mut self, v: &Value, adt: &Adt, variants: &[Variant], ty: &Ty) {
        let item = adt.item.item();
        let live: Vec<(usize, &Variant)> = variants.iter().enumerate().filter(|(_, var)| !var.serde.skip).collect();
        if item.serde.untagged {
            return self.untagged(v, adt, &live);
        }
        if let Some(tag) = &item.serde.tag {
            return self.tagged(v, adt, &live, tag, item.serde.content.as_deref(), ty);
        }
        match &v.kind {
            ValueKind::Named { name, body } => {
                let found =
                    live.iter().find(|(_, var)| variant_name(adt, var) == name.name || var.serde.aliases.contains(&name.name));
                match found {
                    Some(&(i, var)) => {
                        self.record_name(name.span, &adt.item, Some(i));
                        self.variant_body(adt, i, var, body.as_ref(), name.span);
                    }
                    None => {
                        if body.is_none() && live.iter().any(|(_, var)| var.serde.other) {
                            return;
                        }
                        let names: Vec<String> = live.iter().map(|(_, var)| variant_name(adt, var)).collect();
                        let hint = match case::suggest(&name.name, names.iter().map(String::as_str)) {
                            Some(s) => format!("; did you mean `{s}`?"),
                            None if names.len() <= 8 => format!(
                                "; expected {}",
                                names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
                            ),
                            None => String::new(),
                        };
                        let message = format!("unknown variant `{}` of enum `{}`{hint}", name.name, item.name());
                        self.error(C::UnknownVariant, message, name.span);
                    }
                }
            }
            _ => self.mismatch(v, ty),
        }
    }

    fn variant_body(&mut self, adt: &Adt, idx: usize, var: &Variant, body: Option<&Container>, name_span: ByteSpan) {
        let item = adt.item.item();
        let label = format!("{}::{}", item.name(), var.name.name);
        match (&var.fields, body) {
            (Fields::Unit, None) => {}
            (Fields::Unit, Some(c)) => {
                self.error(C::TypeMismatch, format!("`{label}` is a unit variant and takes no value"), c.open);
            }
            (_, None) => {
                let message = format!("`{label}` needs a value: `{}(...)`", variant_name(adt, var));
                self.error(C::TypeMismatch, message, name_span);
            }
            (Fields::Tuple(fs), Some(c)) if fs.len() == 1 => {
                let inner = self.low.field_ty(adt, &fs[0]);
                if self.ext.unwrap_variant_newtypes && self.unwrapped(c, &inner) {
                    return;
                }
                self.positional(c, &[inner], &format!("`{label}`"));
            }
            (Fields::Tuple(fs), Some(c)) => {
                let tys: Vec<Ty> = fs.iter().map(|f| self.low.field_ty(adt, f)).collect();
                self.positional(c, &tys, &format!("`{label}`"));
            }
            (Fields::Named(fs), Some(c)) => {
                let rule = var.serde.rename_all.as_deref().or(item.serde.rename_all_fields.as_deref());
                let set = field_set(&self.low, adt, Some(idx), fs, rule, false, item.serde.deny_unknown_fields, &label);
                self.named_fields(c, set, name_span, None);
            }
        }
    }

    /// `unwrap_variant_newtypes`: `Variant(x: 1)` for `Variant(Inner)` where
    /// `Inner` is a struct (or tuple). True when the body was checked that
    /// way.
    fn unwrapped(&mut self, c: &Container, inner: &Ty) -> bool {
        match inner {
            Ty::Adt(a) => {
                let item = a.item.item();
                match &item.kind {
                    ItemKind::Struct { fields: Fields::Named(fs), .. }
                        if c.kind == ContainerKind::Struct || c.entries.is_empty() =>
                    {
                        let name = item.serde.rename.clone().unwrap_or_else(|| item.name().to_string());
                        let set = field_set(
                            &self.low,
                            a,
                            None,
                            fs,
                            item.serde.rename_all.as_deref(),
                            item.serde.default,
                            item.serde.deny_unknown_fields,
                            &name,
                        );
                        self.named_fields(c, set, c.open, None);
                        true
                    }
                    ItemKind::Struct { fields: Fields::Tuple(fs), .. } if fs.len() != 1 && c.entries.len() != 1 => {
                        let tys: Vec<Ty> = fs.iter().map(|f| self.low.field_ty(a, f)).collect();
                        self.positional(c, &tys, &format!("`{}`", item.name()));
                        true
                    }
                    _ => false,
                }
            }
            Ty::Tuple(ts) if c.entries.len() != 1 => {
                self.positional(c, ts, &format!("`{inner}`"));
                true
            }
            _ => false,
        }
    }

    fn untagged(&mut self, v: &Value, adt: &Adt, live: &[(usize, &Variant)]) {
        for &(i, var) in live {
            if self.trial(|t| t.untagged_variant(v, adt, i, var)) == 0 {
                self.untagged_variant(v, adt, i, var);
                return;
            }
        }
        let message = format!("{} matches no variant of untagged enum `{}`", v.describe(), adt.item.item().name());
        self.error(C::TypeMismatch, message, head(v));
    }

    /// A variant's content on its own, as untagged and adjacently tagged
    /// enums write it.
    fn untagged_variant(&mut self, v: &Value, adt: &Adt, idx: usize, var: &Variant) {
        match &var.fields {
            Fields::Unit => {
                if !is_unit(v) {
                    self.mismatch(v, &Ty::Unit);
                }
            }
            Fields::Tuple(fs) if fs.len() == 1 => {
                let inner = self.low.field_ty(adt, &fs[0]);
                self.check(v, &inner);
            }
            Fields::Tuple(fs) => {
                let tys: Vec<Ty> = fs.iter().map(|f| self.low.field_ty(adt, f)).collect();
                match &v.kind {
                    ValueKind::Container(c) => self.positional(c, &tys, &format!("`{}`", var.name.name)),
                    _ => self.mismatch(v, &Ty::Tuple(tys)),
                }
            }
            Fields::Named(fs) => {
                let item = adt.item.item();
                let label = format!("{}::{}", item.name(), var.name.name);
                let rule = var.serde.rename_all.as_deref().or(item.serde.rename_all_fields.as_deref());
                let set = field_set(&self.low, adt, Some(idx), fs, rule, false, item.serde.deny_unknown_fields, &label);
                match &v.kind {
                    ValueKind::Container(c) | ValueKind::Named { body: Some(c), .. } => {
                        self.named_fields(c, set, head(v), None);
                    }
                    _ => {
                        let message = format!("expected the fields of `{label}`, found {}", v.describe());
                        self.error(C::TypeMismatch, message, head(v));
                    }
                }
            }
        }
    }

    /// `#[serde(tag = "t")]`, with `content = "c"` for adjacent tagging:
    /// `(t: "Variant", ...)` or `(t: "Variant", c: ...)`.
    fn tagged(&mut self, v: &Value, adt: &Adt, live: &[(usize, &Variant)], tag: &str, content: Option<&str>, ty: &Ty) {
        let item = adt.item.item();
        let c = match &v.kind {
            ValueKind::Container(c) | ValueKind::Named { body: Some(c), .. } if c.kind == ContainerKind::Struct => c,
            _ => {
                let message = format!("expected {}, written as `({tag}: \"Variant\", ...)`, found {}", label(ty), v.describe());
                self.error(C::TypeMismatch, message, head(v));
                return;
            }
        };
        let field = |name: &str| {
            c.entries.iter().find(|e| matches!(&e.key, Some(EntryKey::Field(id)) if id.name == name))
        };
        let Some(tag_entry) = field(tag) else {
            let message = format!("missing field `{tag}` naming a variant of `{}`", item.name());
            self.error(C::MissingField, message, head(v));
            return;
        };
        let ValueKind::String { decoded: Some(name) } = &tag_entry.value.kind else {
            self.error(C::TypeMismatch, format!("expected `{tag}` to name a variant as a string"), tag_entry.value.span);
            return;
        };
        let Some(&(idx, var)) = live.iter().find(|(_, var)| variant_name(adt, var) == *name || var.serde.aliases.contains(name))
        else {
            let names: Vec<String> = live.iter().map(|(_, var)| variant_name(adt, var)).collect();
            let hint = case::suggest(name, names.iter().map(String::as_str))
                .map(|s| format!("; did you mean `{s}`?"))
                .unwrap_or_default();
            let message = format!("unknown variant `{name}` of enum `{}`{hint}", item.name());
            self.error(C::UnknownVariant, message, tag_entry.value.span);
            return;
        };
        self.record_name(tag_entry.value.span, &adt.item, Some(idx));
        let label = format!("{}::{}", item.name(), var.name.name);
        if let Some(content) = content {
            match (&var.fields, field(content)) {
                (Fields::Unit, None) => {}
                (_, Some(ce)) => self.untagged_variant(&ce.value, adt, idx, var),
                (_, None) => {
                    self.error(C::MissingField, format!("missing field `{content}` holding `{label}`"), head(v));
                }
            }
            return;
        }
        let set = match &var.fields {
            Fields::Named(fs) => {
                let rule = var.serde.rename_all.as_deref().or(item.serde.rename_all_fields.as_deref());
                field_set(&self.low, adt, Some(idx), fs, rule, false, item.serde.deny_unknown_fields, &label)
            }
            Fields::Tuple(fs) if fs.len() == 1 => match self.low.field_ty(adt, &fs[0]) {
                Ty::Adt(inner) => match &inner.item.item().kind {
                    ItemKind::Struct { fields: Fields::Named(ifs), .. } => {
                        let ii = inner.item.item();
                        field_set(&self.low, &inner, None, ifs, ii.serde.rename_all.as_deref(), ii.serde.default, false, &label)
                    }
                    _ => FieldSet { open: Some(Ty::Any), owner_name: label, ..FieldSet::default() },
                },
                _ => FieldSet { open: Some(Ty::Any), owner_name: label, ..FieldSet::default() },
            },
            _ => FieldSet { owner_name: label, ..FieldSet::default() },
        };
        self.named_fields(c, set, head(v), Some(tag));
    }
}

enum Form<'v> {
    Body(&'v Container),
    Bare,
    Wrong,
}

/// The fields a struct body may hold, with renames applied and flattened
/// structs merged in.
#[allow(clippy::too_many_arguments)]
pub fn field_set(
    low: &Lowerer,
    adt: &Adt,
    variant: Option<usize>,
    fields: &[Field],
    rule: Option<&str>,
    container_default: bool,
    deny_unknown: bool,
    owner_name: &str,
) -> FieldSet {
    let mut set = FieldSet { deny_unknown, owner_name: owner_name.to_string(), ..FieldSet::default() };
    collect_fields(low, &mut set, adt, variant, fields, rule, container_default, 0);
    set
}

#[allow(clippy::too_many_arguments)]
fn collect_fields(
    low: &Lowerer,
    set: &mut FieldSet,
    adt: &Adt,
    variant: Option<usize>,
    fields: &[Field],
    rule: Option<&str>,
    container_default: bool,
    depth: u32,
) {
    for (i, f) in fields.iter().enumerate() {
        if f.serde.skip {
            continue;
        }
        let ty = low.field_ty(adt, f);
        if f.serde.flatten {
            let (inner, optional) = match &ty {
                Ty::Option(t) => ((**t).clone(), true),
                t => (t.clone(), false),
            };
            match &inner {
                Ty::Adt(a) if depth < 8 => match &a.item.item().kind {
                    ItemKind::Struct { fields: Fields::Named(fs), .. } => {
                        let item = a.item.item();
                        let default = container_default || optional || f.serde.default || item.serde.default;
                        collect_fields(low, set, a, None, fs, item.serde.rename_all.as_deref(), default, depth + 1);
                    }
                    _ => set.open = Some(Ty::Any),
                },
                Ty::Map(_, _, v) => set.open = Some((**v).clone()),
                _ => set.open = Some(Ty::Any),
            }
            continue;
        }
        let raw = f.name.as_ref().map_or(String::new(), |n| n.name.clone());
        let name = f.serde.rename.clone().unwrap_or_else(|| rule.map_or(raw.clone(), |r| case::rename_field(r, &raw)));
        let required = !(container_default || f.serde.default || ty.is_option());
        set.fields.push(Expected {
            name,
            aliases: f.serde.aliases.clone(),
            ty,
            required,
            owner: adt.item.clone(),
            variant,
            index: i,
        });
    }
}

/// The name RON writes for a variant.
pub fn variant_name(adt: &Adt, var: &Variant) -> String {
    let item = adt.item.item();
    var.serde
        .rename
        .clone()
        .unwrap_or_else(|| item.serde.rename_all.as_deref().map_or(var.name.name.clone(), |r| case::rename_variant(r, &var.name.name)))
}

/// "struct `Window`", "enum `Mode`", or "`u32`".
pub fn label(ty: &Ty) -> String {
    match ty {
        Ty::Adt(a) => format!("{} `{ty}`", a.item.item().keyword()),
        _ => format!("`{ty}`"),
    }
}

/// Where to point at a value: its name, its opening bracket, or all of it.
fn head(v: &Value) -> ByteSpan {
    match &v.kind {
        ValueKind::Named { name, .. } => name.span,
        ValueKind::Container(c) => c.open,
        _ => v.span,
    }
}

fn is_unit(v: &Value) -> bool {
    matches!(&v.kind, ValueKind::Container(c) if c.kind == ContainerKind::Tuple && c.entries.is_empty())
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

/// A RON integer literal's sign and magnitude: `-0x_FF`, `1_000u16`.
fn parse_ron_int(raw: &str) -> Option<(bool, u128)> {
    let (negative, rest) = match raw.as_bytes().first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let (radix, digits) = if let Some(r) = rest.strip_prefix("0x") {
        (16, r)
    } else if let Some(r) = rest.strip_prefix("0o") {
        (8, r)
    } else if let Some(r) = rest.strip_prefix("0b") {
        (2, r)
    } else {
        (10, rest)
    };
    let digits = digits.split(['i', 'u']).next().unwrap_or("");
    let clean: String = digits.chars().filter(|&c| c != '_').collect();
    u128::from_str_radix(&clean, radix).ok().map(|m| (negative, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_ints() {
        assert_eq!(parse_ron_int("-0x_FF"), Some((true, 255)));
        assert_eq!(parse_ron_int("1_000u16"), Some((false, 1000)));
        assert_eq!(parse_ron_int("0b101i8"), Some((false, 5)));
        assert_eq!(parse_ron_int("+7"), Some((false, 7)));
    }
}
