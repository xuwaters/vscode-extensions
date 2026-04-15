import MarkdownIt from 'markdown-it';
import markdownItFrontMatter from 'markdown-it-front-matter';
import markdownItTaskLists from 'markdown-it-task-lists';
import markdownItTexmath from 'markdown-it-texmath';
import katex from 'katex';
import type Token from 'markdown-it/lib/token.mjs';

export type BlockType =
  | 'paragraph'
  | 'heading'
  | 'code_block'
  | 'blockquote'
  | 'list'
  | 'table'
  | 'hr'
  | 'html_block'
  | 'blank'
  | 'frontmatter'
  | 'mermaid'
  | 'math_block'
  | 'mdx_component';

export interface BlockRange {
  type: BlockType;
  startLine: number;
  endLine: number; // exclusive
}

export interface TextChange {
  rangeOffset: number;
  rangeLength: number;
  text: string;
}

export class DocumentModel {
  private lines: string[] = [];
  private blockMap: BlockRange[] = [];
  private md: MarkdownIt;
  private parsedFrontmatter: string | null = null;
  private isMdx = false;

  constructor() {
    this.md = this.createParser();
  }

  private createParser(): MarkdownIt {
    const md = new MarkdownIt({
      html: true,
      linkify: true,
      typographer: false,
    });

    md.use(markdownItTaskLists, { enabled: true });

    md.use(markdownItFrontMatter, (fm: string) => {
      this.parsedFrontmatter = fm;
    });

    md.use(markdownItTexmath, {
      engine: katex,
      delimiters: 'dollars',
    });

    return md;
  }

  /** Initialize the model with full document content. */
  init(content: string, uri: string): void {
    this.isMdx = /\.mdx$/i.test(uri);
    this.lines = content.split('\n');
    this.parseAll();
  }

  /** Get all lines joined as a single string. */
  getFullText(): string {
    return this.lines.join('\n');
  }

  /** Get a specific line by index. */
  getLine(index: number): string {
    return this.lines[index] ?? '';
  }

  /** Get total line count. */
  getLineCount(): number {
    return this.lines.length;
  }

  /** Get the raw text for a block range. */
  getBlockText(block: BlockRange): string {
    return this.lines.slice(block.startLine, block.endLine).join('\n');
  }

  /** Get the block containing a given line. */
  getBlockForLine(line: number): BlockRange | null {
    for (const block of this.blockMap) {
      if (line >= block.startLine && line < block.endLine) {
        return block;
      }
    }
    return null;
  }

  /** Get all blocks. */
  getBlocks(): BlockRange[] {
    return this.blockMap;
  }

  /** Get the block index for a given line. Returns -1 if not found. */
  getBlockIndex(line: number): number {
    return this.blockMap.findIndex(
      (b) => line >= b.startLine && line < b.endLine,
    );
  }

  /** Render the entire document as HTML. */
  renderAll(): string {
    this.parsedFrontmatter = null;
    const html = this.md.render(this.getFullText());
    return this.wrapWithFrontmatter(html);
  }

  /** Render a single block as HTML. */
  renderBlock(block: BlockRange): string {
    if (block.type === 'frontmatter') {
      return this.renderFrontmatterCard(
        this.lines.slice(block.startLine, block.endLine).join('\n'),
      );
    }
    if (block.type === 'mermaid') {
      const code = this.lines
        .slice(block.startLine + 1, block.endLine - 1)
        .join('\n');
      return `<div class="mermaid-container" data-mermaid-source="${encodeURIComponent(code)}"><pre class="mermaid">${escapeHtml(code)}</pre></div>`;
    }
    const text = this.getBlockText(block);
    return this.md.render(text);
  }

  /** Apply an edit by replacing lines and re-parsing. */
  applyEdit(startLine: number, endLine: number, newLines: string[]): void {
    this.lines.splice(startLine, endLine - startLine, ...newLines);
    this.parseAll();
  }

  /** Apply offset-based changes from the host. */
  applyChanges(changes: TextChange[]): void {
    let fullText = this.getFullText();
    // Apply changes in reverse order to preserve offsets
    const sorted = [...changes].sort((a, b) => b.rangeOffset - a.rangeOffset);
    for (const change of sorted) {
      fullText =
        fullText.slice(0, change.rangeOffset) +
        change.text +
        fullText.slice(change.rangeOffset + change.rangeLength);
    }
    this.lines = fullText.split('\n');
    this.parseAll();
  }

  /** Re-parse the full document and rebuild the block map. */
  private parseAll(): void {
    this.parsedFrontmatter = null;
    const fullText = this.getFullText();
    const tokens = this.md.parse(fullText, {});
    this.blockMap = this.buildBlockMap(tokens);
  }

  private buildBlockMap(tokens: Token[]): BlockRange[] {
    const blocks: BlockRange[] = [];
    const totalLines = this.lines.length;

    // Detect frontmatter (lines 0..N if present)
    if (this.parsedFrontmatter !== null) {
      // Find the closing ---
      let fmEnd = 1;
      while (fmEnd < totalLines && this.lines[fmEnd].trim() !== '---') {
        fmEnd++;
      }
      fmEnd++; // include the closing ---
      blocks.push({ type: 'frontmatter', startLine: 0, endLine: fmEnd });
    }

    for (const token of tokens) {
      if (!token.map) continue;
      const [startLine, endLine] = token.map;

      // Skip tokens whose range is inside an already-added block
      if (
        blocks.length > 0 &&
        startLine >= blocks[blocks.length - 1].startLine &&
        endLine <= blocks[blocks.length - 1].endLine
      ) {
        continue;
      }

      const type = this.classifyToken(token, startLine);
      blocks.push({ type, startLine, endLine });
    }

    // Post-process: detect MDX blocks if this is an .mdx file
    if (this.isMdx) {
      this.classifyMdxBlocks(blocks);
    }

    // Sort blocks by start line
    blocks.sort((a, b) => a.startLine - b.startLine);

    // Fill gaps with blank blocks
    return this.fillGaps(blocks, totalLines);
  }

  private classifyToken(token: Token, startLine: number): BlockType {
    switch (token.type) {
      case 'heading_open':
        return 'heading';
      case 'paragraph_open':
        return 'paragraph';
      case 'fence':
        // Check for mermaid code blocks
        if (token.info.trim().toLowerCase() === 'mermaid') {
          return 'mermaid';
        }
        return 'code_block';
      case 'code_block':
        return 'code_block';
      case 'blockquote_open':
        return 'blockquote';
      case 'bullet_list_open':
      case 'ordered_list_open':
        return 'list';
      case 'table_open':
        return 'table';
      case 'hr':
        return 'hr';
      case 'html_block':
        return 'html_block';
      case 'math_block':
        return 'math_block';
      default:
        // Default to paragraph for unrecognized tokens with line maps
        if (token.type.endsWith('_open')) {
          return 'paragraph';
        }
        return 'paragraph';
    }
  }

  private classifyMdxBlocks(blocks: BlockRange[]): void {
    for (const block of blocks) {
      if (block.type === 'html_block') {
        const firstLine = this.lines[block.startLine]?.trim() ?? '';
        if (/^<[A-Z]/.test(firstLine)) {
          (block as { type: BlockType }).type = 'mdx_component';
        }
      }
      if (block.type === 'paragraph') {
        const firstLine = this.lines[block.startLine]?.trim() ?? '';
        if (/^(import|export)\s/.test(firstLine)) {
          (block as { type: BlockType }).type = 'mdx_component';
        }
      }
    }
  }

  private fillGaps(blocks: BlockRange[], totalLines: number): BlockRange[] {
    if (blocks.length === 0) {
      if (totalLines > 0) {
        return [{ type: 'paragraph', startLine: 0, endLine: totalLines }];
      }
      return [];
    }

    const result: BlockRange[] = [];
    let lastEnd = 0;

    for (const block of blocks) {
      if (block.startLine > lastEnd) {
        // Check if the gap is just blank lines
        const isBlank = this.lines
          .slice(lastEnd, block.startLine)
          .every((l) => l.trim() === '');
        if (!isBlank) {
          result.push({
            type: 'paragraph',
            startLine: lastEnd,
            endLine: block.startLine,
          });
        }
      }
      result.push(block);
      lastEnd = Math.max(lastEnd, block.endLine);
    }

    // Trailing content
    if (lastEnd < totalLines) {
      const isBlank = this.lines
        .slice(lastEnd, totalLines)
        .every((l) => l.trim() === '');
      if (!isBlank) {
        result.push({
          type: 'paragraph',
          startLine: lastEnd,
          endLine: totalLines,
        });
      }
    }

    return result;
  }

  private wrapWithFrontmatter(html: string): string {
    if (this.parsedFrontmatter === null) return html;
    return this.renderFrontmatterCard(`---\n${this.parsedFrontmatter}\n---`) + html;
  }

  private renderFrontmatterCard(raw: string): string {
    // Extract key-value pairs from YAML (simple parsing)
    const lines = raw.split('\n').filter(
      (l) => l.trim() !== '---' && l.trim() !== '',
    );
    const entries: Array<[string, string]> = [];
    for (const line of lines) {
      const match = line.match(/^(\w[\w\s]*?):\s*(.*)$/);
      if (match) {
        entries.push([match[1].trim(), match[2].trim()]);
      }
    }

    if (entries.length === 0) {
      return `<div class="frontmatter-card"><div class="frontmatter-label">Frontmatter</div><pre>${escapeHtml(raw)}</pre></div>`;
    }

    const dl = entries
      .map(([k, v]) => `<dt>${escapeHtml(k)}</dt><dd>${escapeHtml(v)}</dd>`)
      .join('');
    return `<div class="frontmatter-card"><div class="frontmatter-label">Frontmatter</div><dl>${dl}</dl></div>`;
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
