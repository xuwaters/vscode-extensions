//! What search costs, next to `str::find` (what patterns were before they
//! were regular) and the `regex` crate (what they could have been). Asserts
//! nothing — it prints, so run it deliberately:
//!
//! ```sh
//! cargo test -p vim-engine --release --test perf -- --ignored --nocapture
//! ```
//!
//! On an M-series mac, over 20k lines / 1.1 MB, whole-buffer scans land near
//! 0.3 ms for us against 0.6–0.9 ms for the `regex` crate: line-level
//! rejection by required literal (see regex.rs) does the heavy lifting, and
//! what survives it is one line's worth of column scanning. The gap that
//! remains is `:%s` over a buffer where most lines *do* match — ~9× the
//! `regex` crate, since every match allocates its capture strings. That is a
//! one-shot command, not a keystroke, so it has been left alone.
//!
//! The number worth watching is the backtracking one that no literal can
//! filter. A backtracking VM has no linear-time guarantee to fall back on,
//! so that case cost 3.7 s here — a frozen editor — until the operation
//! budget in search.rs bounded it to ~0.1 s and an honest "gave up".

use std::time::Instant;
use vim_engine::buffer::{Buffer, Pos};
use vim_engine::search::{self, Pattern};

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
    for line in 0..buf.line_count() {
        hits += pat.find_all(buf.line(line), true).len();
    }
    println!("  vim-engine  find_all all lines {} {hits} hits", ms(t));
    let re = regex::Regex::new(r"value_\d+").unwrap();
    let t = Instant::now();
    let mut hits = 0;
    for line in 0..buf.line_count() {
        hits += re.find_iter(buf.line(line)).count();
    }
    println!("  regex crate find_iter          {} {hits} hits", ms(t));

    println!("\n== per-line overhead (allocation) ==");
    let t = Instant::now();
    let mut n = 0;
    for line in 0..buf.line_count() {
        n += buf.line(line).chars().count();
    }
    println!("  chars() over every line        {} {n} chars", ms(t));
}
