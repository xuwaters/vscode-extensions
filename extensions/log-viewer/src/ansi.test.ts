import { describe, expect, it } from 'vitest';
import { ansiToHtml, stripAnsi } from './ansi.js';

const ESC = '\x1b';

describe('ansiToHtml', () => {
  it('passes plain text through with HTML escaping', () => {
    expect(ansiToHtml('hello <world> & co')).toBe('hello &lt;world&gt; &amp; co');
  });

  it('renders foreground color', () => {
    const html = ansiToHtml(`${ESC}[31merror${ESC}[0m`);
    expect(html).toContain('color:var(--vscode-terminal-ansiRed)');
    expect(html).toContain('>error</span>');
  });

  it('renders bright background and bold', () => {
    const html = ansiToHtml(`${ESC}[1;103mwarn${ESC}[0m`);
    expect(html).toContain('font-weight:bold');
    expect(html).toContain('background-color:var(--vscode-terminal-ansiBrightYellow)');
  });

  it('handles 256-color foreground', () => {
    const html = ansiToHtml(`${ESC}[38;5;202mx${ESC}[0m`);
    // idx 202: n=186, r=5,g=1,b=0 → rgb(55+5*40, 55+1*40, 0) = rgb(255,95,0)
    expect(html).toContain('color:rgb(255,95,0)');
  });

  it('handles 24-bit truecolor', () => {
    const html = ansiToHtml(`${ESC}[38;2;12;34;56mx${ESC}[0m`);
    expect(html).toContain('color:rgb(12,34,56)');
  });

  it('persists state across newlines until reset', () => {
    const html = ansiToHtml(`${ESC}[31mline1\nline2${ESC}[0m`);
    // Both lines should be inside a red span (with a newline in the slice)
    expect(html).toMatch(/color:var\(--vscode-terminal-ansiRed\).*line1\nline2/);
  });

  it('strips OSC hyperlink sequences', () => {
    const link = `${ESC}]8;;https://example.com${ESC}\\click${ESC}]8;;${ESC}\\`;
    expect(stripAnsi(link)).toBe('click');
    expect(ansiToHtml(link)).toBe('click');
  });

  it('strips OSC sequences terminated by BEL', () => {
    const osc = `${ESC}]0;window title\x07hello`;
    expect(stripAnsi(osc)).toBe('hello');
  });

  it('strips non-SGR CSI sequences (cursor moves, erase)', () => {
    // ESC[2K = erase line, ESC[H = cursor home, ESC[?25l = hide cursor
    const noisy = `${ESC}[2K${ESC}[H${ESC}[?25lvisible`;
    expect(stripAnsi(noisy)).toBe('visible');
    expect(ansiToHtml(noisy)).toBe('visible');
  });

  it('drops bare carriage returns (TTY redraws)', () => {
    expect(stripAnsi('progress: 50%\rprogress: 100%\n')).toBe(
      'progress: 50%progress: 100%\n',
    );
  });

  it('preserves CRLF as a single newline', () => {
    expect(stripAnsi('a\r\nb')).toBe('a\nb');
  });

  it('returns reset segments as plain HTML (no extraneous spans)', () => {
    const html = ansiToHtml(`plain${ESC}[31mred${ESC}[0mafter`);
    expect(html).toBe(
      `plain<span style="color:var(--vscode-terminal-ansiRed)">red</span>after`,
    );
  });

  it('handles empty SGR (ESC[m) as reset', () => {
    const html = ansiToHtml(`${ESC}[31mred${ESC}[mback`);
    expect(html).toBe(
      `<span style="color:var(--vscode-terminal-ansiRed)">red</span>back`,
    );
  });

  it('renders inverse by swapping fg and bg', () => {
    const html = ansiToHtml(`${ESC}[31;42;7mtext${ESC}[0m`);
    // fg becomes ansiGreen (was bg), bg becomes ansiRed (was fg)
    expect(html).toContain('color:var(--vscode-terminal-ansiGreen)');
    expect(html).toContain('background-color:var(--vscode-terminal-ansiRed)');
  });

  it('strips charset designators', () => {
    const sgrInjected = `${ESC}(B${ESC})0hello`;
    expect(stripAnsi(sgrInjected)).toBe('hello');
  });
});
