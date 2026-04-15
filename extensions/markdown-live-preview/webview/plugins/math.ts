import katex from 'katex';

/** Render a LaTeX expression to HTML using KaTeX. */
export function renderMath(latex: string, displayMode: boolean): string {
  try {
    return katex.renderToString(latex, {
      displayMode,
      throwOnError: false,
      output: 'htmlAndMathml',
      trust: false,
    });
  } catch {
    return `<span class="math-error">${escapeHtml(latex)}</span>`;
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
