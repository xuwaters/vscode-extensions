# Diesel schema.rs

Editor support for Diesel's generated `schema.rs`: an outline of your tables
and their columns, hover cards that list every column with its SQL type,
completion inside `diesel::joinable!` and
`diesel::allow_tables_to_appear_in_same_query!`, and diagnostics for
references that don't resolve. The analyzer is Rust compiled to WebAssembly
and runs inside the extension host — no language server, no toolchain to
install.

It works on top of rust-analyzer rather than replacing it: only the three
diesel macros are parsed, everything else in the file is ignored.

```rust
diesel::table! {
    users (id) {
        id -> Text,
        email -> Citext,
        password_hash -> Nullable<Text>,
        created_at_ms -> Int8,
    }
}

diesel::table! {
    messages (id) {
        id -> Text,
        author_id -> Text,
        body -> Text,
    }
}

diesel::joinable!(messages -> users (author_id));

diesel::allow_tables_to_appear_in_same_query!(
    users,
    messages,
);
```

## Features

- **Outline and breadcrumbs**: each `diesel::table!` is published as a
  document symbol with its primary key as the detail (`pk: id`, or
  `pk: channel_id, user_id` for a composite key), and its columns nested
  underneath, each showing its SQL type. Every `joinable!` appears as
  `messages → users`, detailed with the foreign-key column it joins on, and
  every allow-group as `allow_tables_to_appear_in_same_query #1` with a
  count of the tables it lists. Go to Symbol
  in File, the Outline view and breadcrumbs all work over a schema file.

- **Hover**. On a table name — in the `table!` header, in a `joinable!`, or
  in an allow-group list — you get the table's primary key and a markdown
  table of every column and its type. On a column name you get
  `users.email: Citext`. The foreign-key column inside
  `joinable!(messages -> users (author_id))` is resolved against the child
  table, so you see its real SQL type there too; if the column doesn't exist
  the hover says so. Primary-key names and the schema qualifier of an
  `auth.users` style table also hover.

- **Completion** in the two reference macros. In the `child -> parent` part
  of a `joinable!`, the table names defined in the file, labelled
  `diesel table`. Inside the trailing `(fk_column)` parentheses, the columns
  of the child table, with the SQL type as the detail. Inside
  `allow_tables_to_appear_in_same_query!`, the table names you haven't
  listed yet. Nothing is offered anywhere else in the file.

- **Diagnostics** for the mistakes that a hand-edited `schema.rs` collects:
  unknown or duplicated tables and columns, primary keys that aren't
  declared, and `joinable!` pairs missing from any allow-group. See the
  table below. They refresh when a file is opened and on save, and — unless
  you turn `dieselSchema.diagnostics.onType` off — 200 ms after you stop
  typing.

- **Folding** ranges for each multi-line `table!` body and each multi-line
  `allow_tables_to_appear_in_same_query!` invocation.

The parser is tolerant of how the file is written: the `diesel::` prefix is
optional, the macro body may use braces or parentheses, `use` items and
attributes such as `#[sql_name = "..."]` inside the macro are skipped,
table names may be schema-qualified (`auth.users`), primary keys may be
composite, and column types may be nested generics like
`Array<Nullable<Text>>`. Anything that isn't one of the three macros —
including a string literal that happens to contain `diesel::table!` — is
left alone.

## Diagnostics

| Code | Severity | Reported when |
| --- | --- | --- |
| `DS001` | Warning | A `table!` body doesn't parse — no `{`/`(` after the macro, or a missing column block or closing brace |
| `DS002` | Warning | Two `table!` blocks declare the same table name |
| `DS003` | Warning | A column appears twice in one table body |
| `DS004` | Warning | A primary-key column isn't in the table's column list |
| `DS005` | Warning | `joinable!` names a table that isn't defined in the file |
| `DS006` | Warning | `joinable!` names a column the child table doesn't have |
| `DS007` | Warning | `allow_tables_to_appear_in_same_query!` names an unknown table |
| `DS008` | Info | A table is listed twice in one allow-group |
| `DS009` | Info | Two tables are `joinable!` but never appear together in an allow-group (only checked when the file has at least one allow-group) |

Analysis is per file. A `joinable!` that points at a table defined in a
different module is reported as unknown, since the analyzer only sees the
document you have open.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `dieselSchema.enabled` | `true` | Enable the diesel schema analyzer for Rust files containing `diesel::table!` invocations |
| `dieselSchema.diagnostics.enabled` | `true` | Emit diagnostics for unknown table/column references, duplicates, and joinable/allow-group mismatches |
| `dieselSchema.diagnostics.onType` | `true` | Recompute diagnostics as you type, debounced by 200 ms. Turn off to recompute only on open and save |
| `dieselSchema.completion.enabled` | `true` | Suggest table names inside `diesel::joinable!` / `diesel::allow_tables_to_appear_in_same_query!`, and FK column names for the child table |

`dieselSchema.enabled` is read once when the extension starts, so changing
it takes effect after a window reload. The other three apply immediately.

## When it runs

The extension activates on Rust files and then does real work only for
files saved on disk that contain `diesel::table!` — or a bare `table!`
followed by `{` or `(`. Ordinary Rust files in a project that uses Diesel
are untouched, and no diagnostics, symbols, hovers or completions are
produced for them.
