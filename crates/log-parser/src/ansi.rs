// ANSI escape sequence → HTML renderer.
//
// Handles SGR (Select Graphic Rendition) color/style codes and quietly strips
// other CSI sequences (cursor moves, screen clears) and OSC sequences
// (window titles, hyperlinks). Colors map to VS Code's --vscode-terminal-ansi*
// CSS variables so they follow the active theme.

use std::fmt::Write as _;

const ANSI_NAMES: [&str; 8] = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
];

#[derive(Clone, Default)]
struct SgrState {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    strike: bool,
}

impl SgrState {
    fn is_default(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && !self.bold
            && !self.dim
            && !self.italic
            && !self.underline
            && !self.inverse
            && !self.strike
    }

    fn reset(&mut self) {
        *self = SgrState::default();
    }
}

fn ansi_named(index: usize, bright: bool) -> String {
    let prefix = if bright { "Bright" } else { "" };
    format!(
        "var(--vscode-terminal-ansi{}{})",
        prefix, ANSI_NAMES[index]
    )
}

fn ansi_256(idx: u32) -> Option<String> {
    if idx > 255 {
        return None;
    }
    if idx < 8 {
        return Some(ansi_named(idx as usize, false));
    }
    if idx < 16 {
        return Some(ansi_named((idx - 8) as usize, true));
    }
    if idx >= 232 {
        let v = 8 + (idx - 232) * 10;
        return Some(format!("rgb({},{},{})", v, v, v));
    }
    let n = idx - 16;
    let r = n / 36;
    let g = (n % 36) / 6;
    let b = n % 6;
    let conv = |c: u32| if c == 0 { 0 } else { 55 + c * 40 };
    Some(format!("rgb({},{},{})", conv(r), conv(g), conv(b)))
}

fn apply_sgr(state: &mut SgrState, params: &[u32]) {
    let ps: &[u32] = if params.is_empty() { &[0] } else { params };
    let mut i = 0;
    while i < ps.len() {
        let p = ps[i];
        match p {
            0 => state.reset(),
            1 => state.bold = true,
            2 => state.dim = true,
            3 => state.italic = true,
            4 => state.underline = true,
            7 => state.inverse = true,
            9 => state.strike = true,
            22 => {
                state.bold = false;
                state.dim = false;
            }
            23 => state.italic = false,
            24 => state.underline = false,
            27 => state.inverse = false,
            29 => state.strike = false,
            30..=37 => state.fg = Some(ansi_named((p - 30) as usize, false)),
            38 => {
                let mode = ps.get(i + 1).copied();
                if mode == Some(5) {
                    if let Some(c) = ps.get(i + 2).and_then(|&n| ansi_256(n)) {
                        state.fg = Some(c);
                    }
                    i += 2;
                } else if mode == Some(2) {
                    let r = ps.get(i + 2).copied().unwrap_or(0);
                    let g = ps.get(i + 3).copied().unwrap_or(0);
                    let b = ps.get(i + 4).copied().unwrap_or(0);
                    state.fg = Some(format!("rgb({},{},{})", r, g, b));
                    i += 4;
                }
            }
            39 => state.fg = None,
            40..=47 => state.bg = Some(ansi_named((p - 40) as usize, false)),
            48 => {
                let mode = ps.get(i + 1).copied();
                if mode == Some(5) {
                    if let Some(c) = ps.get(i + 2).and_then(|&n| ansi_256(n)) {
                        state.bg = Some(c);
                    }
                    i += 2;
                } else if mode == Some(2) {
                    let r = ps.get(i + 2).copied().unwrap_or(0);
                    let g = ps.get(i + 3).copied().unwrap_or(0);
                    let b = ps.get(i + 4).copied().unwrap_or(0);
                    state.bg = Some(format!("rgb({},{},{})", r, g, b));
                    i += 4;
                }
            }
            49 => state.bg = None,
            90..=97 => state.fg = Some(ansi_named((p - 90) as usize, true)),
            100..=107 => state.bg = Some(ansi_named((p - 100) as usize, true)),
            _ => {}
        }
        i += 1;
    }
}

fn style_for(s: &SgrState) -> String {
    let (fg, bg) = if s.inverse {
        (
            s.bg.clone()
                .or_else(|| Some("var(--vscode-terminal-background)".to_string())),
            s.fg.clone()
                .or_else(|| Some("var(--vscode-terminal-foreground)".to_string())),
        )
    } else {
        (s.fg.clone(), s.bg.clone())
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = fg {
        parts.push(format!("color:{}", c));
    }
    if let Some(c) = bg {
        parts.push(format!("background-color:{}", c));
    }
    if s.bold {
        parts.push("font-weight:bold".to_string());
    }
    if s.dim {
        parts.push("opacity:0.7".to_string());
    }
    if s.italic {
        parts.push("font-style:italic".to_string());
    }
    let mut deco: Vec<&str> = Vec::new();
    if s.underline {
        deco.push("underline");
    }
    if s.strike {
        deco.push("line-through");
    }
    if !deco.is_empty() {
        parts.push(format!("text-decoration:{}", deco.join(" ")));
    }
    parts.join(";")
}

fn html_escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// Strip OSC / DCS / SOS / PM / APC, common single-byte Fe escapes, and charset
/// designators. CSI (ESC [ ...) is handled by the SGR loop.
fn strip_non_csi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != 0x1b {
            // Push bytes verbatim — input is UTF-8; we only branch on ASCII.
            out.push(b as char);
            i += 1;
            continue;
        }
        // ESC seen.
        let next = bytes.get(i + 1).copied();
        match next {
            // OSC (]) / DCS (P) / SOS (X) / PM (^) / APC (_): consume until BEL or ESC \
            Some(b']') | Some(b'P') | Some(b'X') | Some(b'^') | Some(b'_') => {
                let mut j = i + 2;
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        j += 1;
                        break;
                    }
                    if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                i = j;
            }
            // Single-byte Fe escapes: ESC 7/8/=/>/c/D/E/H/M/N/O
            Some(b'7') | Some(b'8') | Some(b'=') | Some(b'>') | Some(b'c') | Some(b'D')
            | Some(b'E') | Some(b'H') | Some(b'M') | Some(b'N') | Some(b'O') => {
                i += 2;
            }
            // Charset designators: ESC ( | ) | * | + | - | . | / followed by one printable byte
            Some(b'(') | Some(b')') | Some(b'*') | Some(b'+') | Some(b'-') | Some(b'.')
            | Some(b'/') => {
                if let Some(&c) = bytes.get(i + 2) {
                    if (0x20..=0x7e).contains(&c) {
                        i += 3;
                        continue;
                    }
                }
                i += 2;
            }
            // ESC [ is CSI — leave for the SGR loop.
            Some(b'[') => {
                out.push(b as char);
                i += 1;
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

fn parse_sgr_params(raw: &str) -> Vec<u32> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(';')
        .filter_map(|p| {
            if p.is_empty() {
                Some(0)
            } else {
                p.parse::<u32>().ok()
            }
        })
        .collect()
}

/// Find the next CSI sequence starting at `start`, returning
/// `(esc_index, after_index, params, final_byte)` or None.
fn find_csi(line: &str, start: usize) -> Option<(usize, usize, Vec<u32>, char)> {
    let bytes = line.as_bytes();
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            // Parameter bytes: 0-9 ; ?
            while j < bytes.len() {
                let c = bytes[j];
                if c.is_ascii_digit() || c == b';' || c == b'?' {
                    j += 1;
                } else {
                    break;
                }
            }
            let params_end = j;
            // Intermediate bytes 0x20-0x2f
            while j < bytes.len() && (0x20..=0x2f).contains(&bytes[j]) {
                j += 1;
            }
            // Final byte 0x40-0x7e
            if j < bytes.len() && (0x40..=0x7e).contains(&bytes[j]) {
                let params_str = &line[i + 2..params_end];
                let params = parse_sgr_params(params_str);
                let final_byte = bytes[j] as char;
                return Some((i, j + 1, params, final_byte));
            }
            // Malformed; skip the ESC and continue searching.
            i += 1;
        } else {
            i += 1;
        }
    }
    None
}

#[derive(Clone)]
pub struct Line {
    pub html: String,
    pub text: String,
}

fn render_line(line: &str, state: &mut SgrState) -> Line {
    let mut html = String::with_capacity(line.len() + 16);
    let mut text = String::with_capacity(line.len());
    let mut open_span = false;
    let mut last = 0usize;

    let flush = |slice: &str,
                 state: &SgrState,
                 html: &mut String,
                 text: &mut String,
                 open_span: &mut bool| {
        if slice.is_empty() {
            return;
        }
        text.push_str(slice);
        if state.is_default() {
            html_escape(slice, html);
            return;
        }
        if !*open_span {
            let _ = write!(html, "<span style=\"{}\">", style_for(state));
            *open_span = true;
        }
        html_escape(slice, html);
    };

    while let Some((idx, after, params, final_byte)) = find_csi(line, last) {
        if idx > last {
            flush(
                &line[last..idx],
                state,
                &mut html,
                &mut text,
                &mut open_span,
            );
        }
        if final_byte == 'm' {
            if open_span {
                html.push_str("</span>");
                open_span = false;
            }
            apply_sgr(state, &params);
        }
        // Non-SGR CSI commands silently dropped.
        last = after;
    }
    if last < line.len() {
        flush(
            &line[last..],
            state,
            &mut html,
            &mut text,
            &mut open_span,
        );
    }
    if open_span {
        html.push_str("</span>");
    }
    Line { html, text }
}

fn preprocess(input: &str) -> String {
    let normalized: String = input
        .replace("\r\n", "\n")
        .chars()
        .filter(|&c| c != '\r')
        .collect();
    strip_non_csi(&normalized)
}

/// Parse ANSI text into per-line `(html, text)` pairs; SGR state carries across lines.
pub fn parse_lines(input: &str) -> Vec<Line> {
    let text = preprocess(input);
    let mut state = SgrState::default();
    text.split('\n')
        .map(|line| render_line(line, &mut state))
        .collect()
}

/// Strip all ANSI escape sequences for plain-text output.
pub fn strip_ansi(input: &str) -> String {
    let preprocessed = preprocess(input);
    // Drop every CSI sequence.
    let mut out = String::with_capacity(preprocessed.len());
    let mut last = 0;
    while let Some((idx, after, _, _)) = find_csi(&preprocessed, last) {
        out.push_str(&preprocessed[last..idx]);
        last = after;
    }
    out.push_str(&preprocessed[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const ESC: &str = "\x1b";

    fn html_of(input: &str) -> String {
        parse_lines(input)
            .into_iter()
            .map(|l| l.html)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plain_text_html_escaped() {
        assert_eq!(html_of("hello <world> & co"), "hello &lt;world&gt; &amp; co");
    }

    #[test]
    fn renders_foreground_color() {
        let html = html_of(&format!("{ESC}[31merror{ESC}[0m"));
        assert!(html.contains("color:var(--vscode-terminal-ansiRed)"));
        assert!(html.contains(">error</span>"));
    }

    #[test]
    fn renders_bright_bg_and_bold() {
        let html = html_of(&format!("{ESC}[1;103mwarn{ESC}[0m"));
        assert!(html.contains("font-weight:bold"));
        assert!(html.contains("background-color:var(--vscode-terminal-ansiBrightYellow)"));
    }

    #[test]
    fn handles_256_color_fg() {
        let html = html_of(&format!("{ESC}[38;5;202mx{ESC}[0m"));
        // 202 → r=5,g=1,b=0 → rgb(255,95,0)
        assert!(html.contains("color:rgb(255,95,0)"));
    }

    #[test]
    fn handles_truecolor() {
        let html = html_of(&format!("{ESC}[38;2;12;34;56mx{ESC}[0m"));
        assert!(html.contains("color:rgb(12,34,56)"));
    }

    #[test]
    fn state_persists_across_newlines() {
        let html = html_of(&format!("{ESC}[31mline1\nline2{ESC}[0m"));
        let red = "color:var(--vscode-terminal-ansiRed)";
        assert_eq!(
            html,
            format!(r#"<span style="{red}">line1</span>\n<span style="{red}">line2</span>"#)
                .replace(r"\n", "\n")
        );
    }

    #[test]
    fn strips_osc_hyperlink() {
        let link = format!("{ESC}]8;;https://example.com{ESC}\\click{ESC}]8;;{ESC}\\");
        assert_eq!(strip_ansi(&link), "click");
        assert_eq!(html_of(&link), "click");
    }

    #[test]
    fn strips_osc_terminated_by_bel() {
        let osc = format!("{ESC}]0;window title\x07hello");
        assert_eq!(strip_ansi(&osc), "hello");
    }

    #[test]
    fn strips_non_sgr_csi() {
        let noisy = format!("{ESC}[2K{ESC}[H{ESC}[?25lvisible");
        assert_eq!(strip_ansi(&noisy), "visible");
        assert_eq!(html_of(&noisy), "visible");
    }

    #[test]
    fn drops_bare_carriage_returns() {
        assert_eq!(
            strip_ansi("progress: 50%\rprogress: 100%\n"),
            "progress: 50%progress: 100%\n"
        );
    }

    #[test]
    fn collapses_crlf() {
        assert_eq!(strip_ansi("a\r\nb"), "a\nb");
    }

    #[test]
    fn empty_sgr_resets() {
        let html = html_of(&format!("{ESC}[31mred{ESC}[mback"));
        assert_eq!(
            html,
            r#"<span style="color:var(--vscode-terminal-ansiRed)">red</span>back"#
        );
    }

    #[test]
    fn inverse_swaps_fg_and_bg() {
        let html = html_of(&format!("{ESC}[31;42;7mtext{ESC}[0m"));
        assert!(html.contains("color:var(--vscode-terminal-ansiGreen)"));
        assert!(html.contains("background-color:var(--vscode-terminal-ansiRed)"));
    }

    #[test]
    fn parse_lines_returns_per_line_text() {
        let lines = parse_lines(&format!("{ESC}[31merror\nokay{ESC}[0m"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "error");
        assert_eq!(lines[1].text, "okay");
    }

    #[test]
    fn parse_lines_keeps_empty_lines() {
        let lines = parse_lines("a\n\nb");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].text, "");
        assert_eq!(lines[1].html, "");
    }
}
