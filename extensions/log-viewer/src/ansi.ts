// ANSI escape sequence → HTML renderer.
//
// Handles SGR (Select Graphic Rendition) color/style codes and quietly strips
// other CSI sequences (cursor moves, screen clears) and OSC sequences
// (window titles, hyperlinks). Colors map to VS Code's --vscode-terminal-ansi*
// CSS variables so they follow the active theme.

const ANSI_NAMES = [
  'Black',
  'Red',
  'Green',
  'Yellow',
  'Blue',
  'Magenta',
  'Cyan',
  'White',
] as const;

interface SgrState {
  fg: string | null;
  bg: string | null;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
  inverse: boolean;
  strike: boolean;
}

function defaultState(): SgrState {
  return {
    fg: null,
    bg: null,
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    inverse: false,
    strike: false,
  };
}

function isDefault(s: SgrState): boolean {
  return (
    s.fg === null &&
    s.bg === null &&
    !s.bold &&
    !s.dim &&
    !s.italic &&
    !s.underline &&
    !s.inverse &&
    !s.strike
  );
}

function ansiNamedColor(index: number, bright: boolean): string {
  const name = ANSI_NAMES[index];
  return `var(--vscode-terminal-ansi${bright ? 'Bright' : ''}${name})`;
}

function ansi256(idx: number): string {
  if (idx < 0 || idx > 255) return '';
  if (idx < 8) return ansiNamedColor(idx, false);
  if (idx < 16) return ansiNamedColor(idx - 8, true);
  if (idx >= 232) {
    const v = 8 + (idx - 232) * 10;
    return `rgb(${v},${v},${v})`;
  }
  const n = idx - 16;
  const r = Math.floor(n / 36);
  const g = Math.floor((n % 36) / 6);
  const b = n % 6;
  const conv = (c: number): number => (c === 0 ? 0 : 55 + c * 40);
  return `rgb(${conv(r)},${conv(g)},${conv(b)})`;
}

function applySgr(state: SgrState, params: number[]): void {
  // SGR 0 with no params resets.
  const ps = params.length === 0 ? [0] : params;
  let i = 0;
  while (i < ps.length) {
    const p = ps[i];
    if (p === 0) {
      Object.assign(state, defaultState());
    } else if (p === 1) {
      state.bold = true;
    } else if (p === 2) {
      state.dim = true;
    } else if (p === 3) {
      state.italic = true;
    } else if (p === 4) {
      state.underline = true;
    } else if (p === 7) {
      state.inverse = true;
    } else if (p === 9) {
      state.strike = true;
    } else if (p === 22) {
      state.bold = false;
      state.dim = false;
    } else if (p === 23) {
      state.italic = false;
    } else if (p === 24) {
      state.underline = false;
    } else if (p === 27) {
      state.inverse = false;
    } else if (p === 29) {
      state.strike = false;
    } else if (p >= 30 && p <= 37) {
      state.fg = ansiNamedColor(p - 30, false);
    } else if (p === 38) {
      const mode = ps[i + 1];
      if (mode === 5) {
        const c = ansi256(ps[i + 2] ?? -1);
        if (c) state.fg = c;
        i += 2;
      } else if (mode === 2) {
        const r = ps[i + 2] ?? 0;
        const g = ps[i + 3] ?? 0;
        const b = ps[i + 4] ?? 0;
        state.fg = `rgb(${r},${g},${b})`;
        i += 4;
      }
    } else if (p === 39) {
      state.fg = null;
    } else if (p >= 40 && p <= 47) {
      state.bg = ansiNamedColor(p - 40, false);
    } else if (p === 48) {
      const mode = ps[i + 1];
      if (mode === 5) {
        const c = ansi256(ps[i + 2] ?? -1);
        if (c) state.bg = c;
        i += 2;
      } else if (mode === 2) {
        const r = ps[i + 2] ?? 0;
        const g = ps[i + 3] ?? 0;
        const b = ps[i + 4] ?? 0;
        state.bg = `rgb(${r},${g},${b})`;
        i += 4;
      }
    } else if (p === 49) {
      state.bg = null;
    } else if (p >= 90 && p <= 97) {
      state.fg = ansiNamedColor(p - 90, true);
    } else if (p >= 100 && p <= 107) {
      state.bg = ansiNamedColor(p - 100, true);
    }
    // Unknown codes are ignored.
    i++;
  }
}

function styleFor(s: SgrState): string {
  const fg = s.inverse ? s.bg ?? 'var(--vscode-terminal-background)' : s.fg;
  const bg = s.inverse ? s.fg ?? 'var(--vscode-terminal-foreground)' : s.bg;
  const parts: string[] = [];
  if (fg) parts.push(`color:${fg}`);
  if (bg) parts.push(`background-color:${bg}`);
  if (s.bold) parts.push('font-weight:bold');
  if (s.dim) parts.push('opacity:0.7');
  if (s.italic) parts.push('font-style:italic');
  const decoration: string[] = [];
  if (s.underline) decoration.push('underline');
  if (s.strike) decoration.push('line-through');
  if (decoration.length) parts.push(`text-decoration:${decoration.join(' ')}`);
  return parts.join(';');
}

export function htmlEscape(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

// Strip terminal control sequences (other than CSI, which is handled below).
//   - OSC / DCS / SOS / PM / APC: ESC (]|P|X|^|_) ... (BEL | ESC \)
//   - Common single-byte Fe escapes: ESC 7/8/=/>/c/D/E/H/M/N/O
//   - Charset designators: ESC ( B, ESC ) 0, etc.
function stripNonCsiEscapes(text: string): string {
  return text
    .replace(/\x1b[\]PX^_][\s\S]*?(?:\x07|\x1b\\)/g, '')
    .replace(/\x1b[78=>cDEHMNO]/g, '')
    .replace(/\x1b[()*+\-./][\x20-\x7e]/g, '');
}

// CSI: ESC [ <params> <intermediate> <final 0x40-0x7e>
const CSI_RE = /\x1b\[([0-9;?]*)([\x20-\x2f]*)([\x40-\x7e])/g;

export interface AnsiToHtmlOptions {
  // If true, rebuild a single span per state change. Default true.
  // (Reserved for future tuning.)
  groupSpans?: boolean;
}

export function ansiToHtml(input: string, _opts: AnsiToHtmlOptions = {}): string {
  // Normalize line endings; drop bare CRs (from progress bars / TTY redraws)
  // since we render all output linearly.
  let text = input.replace(/\r\n/g, '\n').replace(/\r/g, '');

  // Strip non-CSI control sequences first; CSI is handled below.
  text = stripNonCsiEscapes(text);

  const state = defaultState();
  let html = '';
  let openSpan = false;
  let lastIdx = 0;

  const flush = (slice: string): void => {
    if (slice.length === 0) return;
    if (isDefault(state)) {
      html += htmlEscape(slice);
      return;
    }
    if (!openSpan) {
      html += `<span style="${styleFor(state)}">`;
      openSpan = true;
    }
    html += htmlEscape(slice);
  };

  const closeSpan = (): void => {
    if (openSpan) {
      html += '</span>';
      openSpan = false;
    }
  };

  CSI_RE.lastIndex = 0;
  for (let m = CSI_RE.exec(text); m !== null; m = CSI_RE.exec(text)) {
    if (m.index > lastIdx) {
      flush(text.slice(lastIdx, m.index));
    }
    const final = m[3];
    if (final === 'm') {
      const params = m[1]
        ? m[1]
            .split(';')
            .map((p) => (p === '' ? 0 : Number.parseInt(p, 10)))
            .filter((n) => !Number.isNaN(n))
        : [];
      // Style change → close previous span; next flush will reopen.
      closeSpan();
      applySgr(state, params);
    }
    // Other CSI commands (cursor move, erase, etc.) are silently dropped.
    lastIdx = CSI_RE.lastIndex;
  }
  if (lastIdx < text.length) {
    flush(text.slice(lastIdx));
  }
  closeSpan();
  return html;
}

// Strip every ANSI escape sequence (SGR + others). Useful for plain-text
// search or copy-to-clipboard.
export function stripAnsi(input: string): string {
  let text = input.replace(/\r\n/g, '\n').replace(/\r/g, '');
  text = stripNonCsiEscapes(text);
  text = text.replace(CSI_RE, '');
  return text;
}
