//! What search costs, next to `str::find` (what patterns were before they
//! were regular) and the `regex` crate (what they could have been). Asserts
//! nothing — it prints, so run it deliberately:
//!
//! ```sh
//! cargo test -p vim-engine --release --test perf -- --ignored --nocapture
//! ```
//!
//! On an M-series mac, over 20k lines / 1.1 MB, whole-buffer scans land near
//! 0.2 ms for us against 0.4–0.6 ms for the `regex` crate: line-level
//! rejection by required literal (see regex.rs) does the heavy lifting, and
//! what survives it is one line's worth of column scanning.
//!
//! `:%s` over a buffer where most lines *do* match is the case with nothing
//! to reject: 1.5 ms of matching against the crate's 0.9 ms, and 3.4 ms end
//! to end — 12k edits, their replacement text, and the mirror brought back
//! in step. Most of what is left is the floor printed at the bottom, deciding
//! a line may match and decomposing it into chars, rather than the VM.
//! Handing 12k edits to the host used to be the bigger number — ~1 MB of
//! escaped JSON, milliseconds to write and more to `JSON.parse` — which is
//! why wasm_api.rs now ships them as one binary block, measured below.
//!
//! The number worth watching is the backtracking one that no literal can
//! filter. A backtracking VM has no linear-time guarantee to fall back on,
//! so that case cost 3.7 s here — a frozen editor — until the operation
//! budget in search.rs bounded it to a fraction of a second and an honest
//! "gave up".

use std::time::Instant;
use vim_engine::buffer::{Buffer, Pos};
use vim_engine::search::{self, Pattern};
use vim_engine::{Key, Session};

/// ~20k lines / ~840 KB of source-shaped text; "needle" only on the last line.
fn corpus() -> String {
    let shapes = [
        "let value_{i} = compute(alpha, beta, gamma);",
        "    if value_{i} > 42 { report(&mut sink, value_{i}); }",
        "// step {i}: fold the accumulator into the running total",
        "pub fn handler_{i}(input: &str) -> Result<Output, Error> {",
        "        self.entries.insert(key_{i}.to_string(), value_{i});",
    ];
    let mut lines: Vec<String> = (0..20_000)
        .map(|i| shapes[i % shapes.len()].replace("{i}", &i.to_string()))
        .collect();
    lines.push("    return needle;".into());
    lines.join("\n")
}

fn ms(t: Instant) -> String {
    format!("{:>9.3?}", t.elapsed())
}

/// Type an ex command and press Enter, as the host feeds keys.
fn type_ex(session: &mut Session, command: &str) -> vim_engine::Effects {
    for ch in command.chars() {
        session.key(Key::Char(ch));
    }
    session.key(Key::Enter)
}

/// The pre-regex implementation: literal text scanned with `str::find`.
fn literal_find(buf: &Buffer, text: &str) -> Option<Pos> {
    for line in 0..buf.line_count() {
        if let Some(byte) = buf.line(line).find(text) {
            return Some(Pos::new(line, vim_engine::buffer::byte_to_utf16(buf.line(line), byte)));
        }
    }
    None
}

/// The same search the regex crate would do line by line.
fn crate_find(buf: &Buffer, re: &regex::Regex) -> Option<Pos> {
    for line in 0..buf.line_count() {
        if let Some(m) = re.find(buf.line(line)) {
            return Some(Pos::new(line, m.start()));
        }
    }
    None
}

/// Compiled and budgeted the way state.rs does it for one keystroke.
fn ours(buf: &Buffer, pattern: &str) -> Option<Pos> {
    let pat = Pattern::parse(pattern).expect("compiles");
    pat.budget_for(buf);
    search::find(buf, Pos::new(0, 0), &pat, false, 1)
}

#[test]
#[ignore = "a benchmark, not a test: run with --ignored --nocapture"]
fn perf() {
    let text = corpus();
    let buf = Buffer::from_text(&text);
    println!(
        "\ncorpus: {} lines, {} KB\n",
        buf.line_count(),
        text.len() / 1024
    );

    println!("== whole-buffer scan (match on the last line) ==");
    let t = Instant::now();
    let a = literal_find(&buf, "needle");
    println!("  str::find   'needle'          {} {a:?}", ms(t));
    let t = Instant::now();
    let b = crate_find(&buf, &regex::Regex::new("needle").unwrap());
    println!("  regex crate 'needle'          {} {b:?}", ms(t));
    let t = Instant::now();
    let c = ours(&buf, "needle");
    println!("  vim-engine  'needle'          {} {c:?}", ms(t));

    println!("\n== whole-buffer scan, no literal prefilter ==");
    let t = Instant::now();
    let b = crate_find(&buf, &regex::Regex::new("[nq]eedle").unwrap());
    println!("  regex crate '[nq]eedle'       {} {b:?}", ms(t));
    let t = Instant::now();
    let c = ours(&buf, "[nq]eedle");
    println!("  vim-engine  '[nq]eedle'       {} {c:?}", ms(t));

    println!("\n== whole-buffer scan, a pattern that backtracks ==");
    let t = Instant::now();
    let b = crate_find(&buf, &regex::Regex::new(".*=.*;zz").unwrap());
    println!("  regex crate '.*=.*;zz'        {} {b:?}", ms(t));
    let t = Instant::now();
    let c = ours(&buf, ".*=.*;zz");
    println!("  vim-engine  '.*=.*;zz'        {} {c:?}", ms(t));

    // Nothing to reject lines with, so this one rides entirely on the budget.
    let t = Instant::now();
    let pat = Pattern::parse(r"\%(\w*\)*\d\{9}").expect("compiles");
    pat.budget_for(&buf);
    let c = search::find(&buf, Pos::new(0, 0), &pat, false, 1);
    println!(
        "  vim-engine  '\\%(\\w*\\)*\\d\\{{9}}'    {} {c:?} gave_up={}",
        ms(t),
        pat.gave_up()
    );

    println!("\n== typical interactive search (hit within ~20 lines) ==");
    for pattern in ["value_9 ", r"\<handler_3\>", r"key_\d\+"] {
        let t = Instant::now();
        let c = ours(&buf, pattern);
        println!("  vim-engine  {pattern:16}  {} {c:?}", ms(t));
    }

    println!("\n== :%s/…/…/g over the whole buffer ==");
    let pat = Pattern::parse(r"value_\d\+").expect("compiles");
    pat.budget_for(&buf);
    let t = Instant::now();
    let mut hits = 0;
    let mut found = Vec::new();
    for line in 0..buf.line_count() {
        pat.find_all_into(buf.line(line), true, &mut found);
        hits += found.len();
    }
    println!("  vim-engine  find_all all lines {} {hits} hits", ms(t));
    // The same scan without a repeat to count, which is what counting one
    // costs on top of plain literal matching.
    let pat = Pattern::parse("value_").expect("compiles");
    pat.budget_for(&buf);
    let t = Instant::now();
    let mut hits = 0;
    for line in 0..buf.line_count() {
        pat.find_all_into(buf.line(line), true, &mut found);
        hits += found.len();
    }
    println!("  vim-engine  no repeat 'value_' {} {hits} hits", ms(t));
    let re = regex::Regex::new(r"value_\d+").unwrap();
    let t = Instant::now();
    let mut hits = 0;
    for line in 0..buf.line_count() {
        hits += re.find_iter(buf.line(line)).count();
    }
    println!("  regex crate find_iter          {} {hits} hits", ms(t));

    // The command as a user runs it: matching, building one edit per line,
    // and bringing the engine's mirror of the document back in step.
    let mut session = Session::new(&text);
    let t = Instant::now();
    let effects = type_ex(&mut session, r":%s/value_\d\+/V/g");
    println!(
        "  :%s/value_\\d\\+/V/g end to end {} {} edits, {:?}",
        ms(t),
        effects.edits.len(),
        effects.message
    );
    // What the host is handed (wasm_api.rs): a small JSON envelope plus the
    // edits as one binary block — not the ~1 MB of escaped JSON it once was.
    let mut ws = vim_engine::wasm_api::Session::new(&text, 0, 0);
    for ch in r":%s/value_\d\+/V/g".chars() {
        ws.key(&ch.to_string());
    }
    let t = Instant::now();
    let envelope = ws.key("<cr>");
    let across = ms(t); // matching + mirror + envelope, as the host waits
    let t = Instant::now();
    let block = ws.take_edits();
    println!(
        "  wasm boundary: key + envelope  {across} {} B of JSON",
        envelope.len()
    );
    println!(
        "  wasm boundary: edit block      {} {} KB binary",
        ms(t),
        block.len() / 1024
    );

    println!("\n== per-line overhead (allocation) ==");
    let t = Instant::now();
    let mut n = 0;
    for line in 0..buf.line_count() {
        n += buf.line(line).chars().count();
    }
    println!("  chars() over every line        {} {n} chars", ms(t));

    // What a scan pays before the VM starts: the literal prefilter over every
    // line, then decomposing the surviving ones and skipping to the columns a
    // match could start at.
    let t = Instant::now();
    let mut n = 0;
    for line in 0..buf.line_count() {
        n += usize::from(buf.line(line).contains("value_"));
    }
    println!("  contains('value_') per line    {} {n} lines", ms(t));
    let t = Instant::now();
    let mut chars: Vec<char> = Vec::new();
    let mut n = 0;
    for line in 0..buf.line_count() {
        let text = buf.line(line);
        if !text.contains("value_") {
            continue;
        }
        chars.clear();
        chars.extend(text.bytes().map(char::from));
        n += chars.iter().filter(|&&c| c == 'v').count();
    }
    println!("  + decompose and skip to 'v'    {} {n} columns", ms(t));
}
