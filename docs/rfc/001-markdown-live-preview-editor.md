# RFC 001: Obsidian-Style Markdown Live Preview Editor for VSCode

**Status**: Draft  
**Date**: 2026-04-15  
**Extension name**: `wx-vsce-markdown-live-preview`

---

## 1. Motivation

VSCode's built-in markdown experience offers two separate modes: a source editor and a read-only preview panel. Users must mentally map between the two. Obsidian popularized a "Live Preview" mode where the document is rendered as formatted text, but the line under the cursor reveals raw markdown syntax for editing. This RFC proposes a VSCode extension that brings this hybrid editing experience to VSCode.

## 2. Design Goals

1. **Three viewing modes** -- The editor supports three distinct modes the user can switch between at any time:
   - **Source mode** -- Raw markdown text in a full CodeMirror editor, like a traditional code editor.
   - **Read mode** -- Fully rendered, read-only preview. No editing; clicking a block does nothing.
   - **Live Preview mode** -- The document renders as formatted markdown. The _block_ containing the cursor reveals raw markdown for editing (Obsidian-style).
2. **Block-level granularity** -- In Live Preview mode, the editing unit is a _block_ (paragraph, heading, code fence, list, table, blockquote, frontmatter, etc.), not a single line. Clicking anywhere in a block activates the entire block for editing. This avoids splitting multi-line constructs (tables, fenced code, lists) at awkward boundaries.
3. **Faithful rendering** -- Headings, bold, italic, links, images, code blocks, tables, task lists, blockquotes, and extended syntax (frontmatter, mermaid diagrams, math/LaTeX, MDX components) all render correctly in preview and read modes.
4. **Zero-friction editing** -- Arrow keys, Enter, Backspace, Tab, and all standard text editing operations work naturally. No modal switching required.
5. **Document fidelity** -- The underlying `.md` / `.mdx` file is the source of truth. The extension never rewrites or reformats content the user didn't touch.
6. **Performance** -- Smooth on documents up to ~10K lines. Incremental re-rendering on edit, not full-document re-parse.
7. **Standard VSCode integration** -- Works with the command palette, file explorer, undo/redo stack, and "Open With..." picker.

## 3. High-Level Architecture

```
┌──────────────────────────────────────────────────────┐
│ VSCode Host                                           │
│                                                        │
│  TextDocument (.md / .mdx file)                        │
│       ▲                                                │
│       │ sync edits                                     │
│       ▼                                                │
│  CustomTextEditorProvider                              │
│       │                                                │
│       │ postMessage / onDidReceiveMessage               │
│       ▼                                                │
│  ┌──────────────────────────────────────────────────┐  │
│  │ Webview                                           │  │
│  │                                                    │  │
│  │  ┌────────────────────────────────────────────┐   │  │
│  │  │ ModeController                             │   │  │
│  │  │  - current mode: source | read | live      │   │  │
│  │  │  - switches renderer on mode change        │   │  │
│  │  └───────┬──────────┬──────────┬──────────────┘   │  │
│  │          │          │          │                    │  │
│  │   ┌──────▼───┐ ┌────▼────┐ ┌──▼──────────────┐   │  │
│  │   │ Source   │ │ Read    │ │ Live Preview     │   │  │
│  │   │ Renderer │ │ Renderer│ │ Renderer         │   │  │
│  │   │          │ │         │ │                   │   │  │
│  │   │ Full-doc │ │ Full-doc│ │ Block preview +  │   │  │
│  │   │ CodeMir. │ │ HTML    │ │ active-block     │   │  │
│  │   │ editor   │ │ render  │ │ CodeMirror       │   │  │
│  │   └──────────┘ └─────────┘ └──────────────────┘   │  │
│  │                                                    │  │
│  │  ┌────────────────────────────────────────────┐   │  │
│  │  │ Document Model (block-indexed)             │   │  │
│  │  │  - parsed AST (markdown-it tokens)         │   │  │
│  │  │  - block map (BlockRange[])                │   │  │
│  │  │  - plugin renderers (mermaid, math, MDX, …) │   │  │
│  │  └────────────────────────────────────────────┘   │  │
│  └──────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

The extension uses VSCode's **Custom Text Editor API** (`CustomTextEditorProvider`). This registers a webview-backed editor for `.md` and `.mdx` files. The webview contains a **ModeController** that manages three rendering strategies sharing a single **DocumentModel**. The extension host bridges the webview and the underlying `TextDocument`.

## 4. Key Components

### 4.1 Extension Host Side

#### `LivePreviewEditorProvider` (implements `CustomTextEditorProvider`)

Responsibilities:
- Register as an editor for `markdown` language ID via `resolveCustomTextEditor`.
- On open: read the full `TextDocument` content, send it to the webview.
- Listen for `onDidChangeTextDocument` events and forward incremental changes to the webview.
- Receive edit operations from the webview and apply them to the `TextDocument` via `WorkspaceEdit`.
- Manage webview lifecycle (HTML scaffold, script/style URIs, CSP nonces).

```typescript
// Registration in extension.ts
vscode.window.registerCustomEditorProvider(
  'markdownLivePreview.editor',
  new LivePreviewEditorProvider(context),
  {
    webviewOptions: { retainContextWhenHidden: true },
    supportsMultipleEditorsPerDocument: false,
  }
);

// Mode cycling command (Source → Live Preview → Read → Source)
vscode.commands.registerCommand('markdownLivePreview.cycleMode', () => { ... });
vscode.commands.registerCommand('markdownLivePreview.setMode', (mode: EditorMode) => { ... });
```

#### Message Protocol (Host ↔ Webview)

| Direction | Message Type | Payload | Description |
|-----------|-------------|---------|-------------|
| Host → Webview | `doc:init` | `{ content: string, uri: string, mode: EditorMode }` | Full document content on open |
| Host → Webview | `doc:update` | `{ changes: TextDocumentContentChangeEvent[] }` | Incremental edits from external sources (e.g., other extensions, git) |
| Webview → Host | `edit:apply` | `{ startLine: number, endLine: number, newText: string }` | User edit from the active block editor |
| Host → Webview | `edit:ack` | `{ version: number }` | Confirm edit applied, send new doc version |
| Webview → Host | `cursor:changed` | `{ line: number }` | Cursor moved to a new block |
| Host → Webview | `mode:set` | `{ mode: EditorMode }` | Switch between source / read / live-preview |
| Webview → Host | `mode:changed` | `{ mode: EditorMode }` | Confirm mode switch completed |
| Host → Webview | `config:update` | `{ fontSize: number, fontFamily: string, theme: string }` | VSCode config changes (theme, font) |

```typescript
type EditorMode = 'source' | 'read' | 'live-preview';
```

### 4.2 Webview Side

#### ModeController

The central coordinator. Owns the document model, manages the active renderer, and handles mode transitions.

```typescript
class ModeController {
  private model: DocumentModel;
  private mode: EditorMode;
  private renderers: Record<EditorMode, Renderer>;
  private container: HTMLElement;

  /** Switch to a new mode. Commits any pending edits, tears down the old renderer, activates the new one. */
  setMode(newMode: EditorMode): void {
    if (newMode === this.mode) return;
    this.renderers[this.mode].teardown();
    this.mode = newMode;
    this.renderers[this.mode].mount(this.container, this.model);
  }
}

interface Renderer {
  mount(container: HTMLElement, model: DocumentModel): void;
  teardown(): void;
  onDocumentChanged(changes: TextChange[]): void;
}
```

Mode transition rules:
- **Any → Source**: Commit pending block edits, destroy preview DOM, create a full-document CodeMirror editor.
- **Any → Read**: Commit pending block edits, destroy editors, render full document as HTML, disable click handlers.
- **Any → Live Preview**: Commit pending block edits, render all blocks as preview HTML, attach click-to-edit handlers.
- Cursor position and scroll offset are preserved across transitions when possible.

#### Document Model (`DocumentModel`)

Maintains the in-memory representation of the markdown document, indexed by block.

```typescript
class DocumentModel {
  private lines: string[];           // raw markdown lines
  private tokens: Token[][];         // markdown-it tokens per block
  private blockMap: BlockRange[];    // maps line ranges to block types

  /** Replace lines and re-parse affected blocks */
  applyEdit(startLine: number, endLine: number, newLines: string[]): void;

  /** Get rendered HTML for a range of lines */
  renderRange(startLine: number, endLine: number): string;

  /** Get raw text for a line */
  getLine(index: number): string;

  /** Get raw text for an entire block */
  getBlockText(block: BlockRange): string;

  /** Get the block range containing a given line */
  getBlockForLine(line: number): BlockRange;

  /** Get all blocks */
  getBlocks(): BlockRange[];
}

interface BlockRange {
  type: 'paragraph' | 'heading' | 'code_block' | 'blockquote'
      | 'list' | 'table' | 'hr' | 'html_block' | 'blank'
      | 'frontmatter' | 'mermaid' | 'math_block' | 'mdx_component';
  startLine: number;
  endLine: number;     // exclusive
}
```

#### Source Renderer (`SourceRenderer`)

A single full-document CodeMirror 6 instance with markdown language support. This is the simplest mode -- essentially a traditional text editor.

```typescript
class SourceRenderer implements Renderer {
  private editor: EditorView | null;

  mount(container: HTMLElement, model: DocumentModel): void {
    const fullText = model.getFullText();
    this.editor = new EditorView({
      doc: fullText,
      extensions: [
        basicSetup,
        markdown(),
        themeExtension,
        EditorView.lineWrapping,
        EditorView.updateListener.of(update => {
          if (update.docChanged) sendEditToHost(update);
        }),
      ],
      parent: container,
    });
  }

  teardown(): void {
    this.editor?.destroy();
    this.editor = null;
  }
}
```

#### Read Renderer (`ReadRenderer`)

Renders the entire document as read-only HTML. No CodeMirror instances. Blocks are non-interactive.

```typescript
class ReadRenderer implements Renderer {
  mount(container: HTMLElement, model: DocumentModel): void {
    container.innerHTML = model.renderAll();
    container.classList.add('read-mode');
    // No click handlers -- purely read-only
  }

  teardown(): void {
    container.classList.remove('read-mode');
    container.innerHTML = '';
  }

  onDocumentChanged(changes: TextChange[]): void {
    // Re-render only affected blocks
  }
}
```

#### Live Preview Renderer (`LivePreviewRenderer`)

The Obsidian-style hybrid renderer. Each block is a `<div>` with `data-line-start` and `data-line-end` attributes.

- **Preview mode** (per block): The block's `innerHTML` is the rendered HTML from markdown-it.
- **Edit mode** (per block): The block is replaced by a CodeMirror 6 editor instance initialized with the raw markdown text for that block.

```typescript
class LivePreviewRenderer implements Renderer {
  private container: HTMLElement;
  private model: DocumentModel;
  private activeBlock: BlockRange | null;
  private activeEditor: EditorView | null;

  /** Full render of all blocks as preview HTML */
  mount(container: HTMLElement, model: DocumentModel): void;

  /** Re-render only changed blocks after an edit */
  renderDirty(changedLines: Set<number>): void;

  /** Activate edit mode for the block containing `line` */
  activateBlock(line: number): void;

  /** Deactivate the current edit block, commit text, return to preview */
  deactivateBlock(): string | null;
}
```

#### Block-Level Edit Mode (Live Preview)

When the user clicks on a rendered block or navigates into it with arrow keys:

1. The renderer calls `deactivateBlock()` on the previously active block (if any), which:
   - Reads the current text from the CodeMirror instance.
   - Compares with the original block text.
   - If changed, sends an `edit:apply` message to the host.
   - Destroys the CodeMirror instance.
   - Re-renders the block as preview HTML.

2. The renderer calls `activateBlock(line)` on the new block, which:
   - Replaces the block's preview HTML with a CodeMirror 6 editor.
   - Initializes CodeMirror with the raw markdown text for that block's lines.
   - Sets cursor position within the block based on the clicked line/column.
   - Focuses the editor.

#### CodeMirror 6 Configuration

Used by both Source Renderer (full-document) and Live Preview Renderer (per-block). The shared base configuration:

```typescript
import { EditorView, basicSetup } from 'codemirror';
import { markdown } from '@codemirror/lang-markdown';
import { oneDark } from '@codemirror/theme-one-dark';

function createEditor(
  parent: HTMLElement,
  content: string,
  opts: {
    cursorLine?: number;
    cursorCol?: number;
    theme: 'light' | 'dark';
    readOnly?: boolean;
  }
): EditorView {
  const view = new EditorView({
    doc: content,
    extensions: [
      basicSetup,
      markdown(),
      opts.theme === 'dark' ? oneDark : [],
      EditorView.lineWrapping,
      opts.readOnly ? EditorState.readOnly.of(true) : [],
      // Custom keymap for block navigation (Live Preview mode only)
      keymap.of(blockNavigationKeymap),
      // Listener to send edits back
      EditorView.updateListener.of(update => {
        if (update.docChanged) {
          onContentChanged(update);
        }
      }),
    ],
    parent,
  });
  if (opts.cursorLine != null) {
    const pos = view.state.doc.line(opts.cursorLine + 1).from + (opts.cursorCol ?? 0);
    view.dispatch({ selection: { anchor: pos } });
  }
  view.focus();
  return view;
}
```

### 4.3 Markdown Parsing

Use **markdown-it** as the core parser. It produces a token stream with source maps (`token.map = [startLine, endLine]`) that maps directly to the `BlockRange[]` index.

```typescript
import MarkdownIt from 'markdown-it';
import markdownItFrontMatter from 'markdown-it-front-matter';
import markdownItTaskLists from 'markdown-it-task-lists';
import markdownItFootnote from 'markdown-it-footnote';
import markdownItTexmath from 'markdown-it-texmath';

const md = new MarkdownIt({
  html: true,
  linkify: true,
  typographer: false,
  highlight: (code, lang) => highlightCode(code, lang),
});

// Core plugins
md.use(markdownItTaskLists, { enabled: true });
md.use(markdownItFootnote);
md.use(markdownItFrontMatter, (fm: string) => { parsedFrontmatter = fm; });
md.use(markdownItTexmath, {
  engine: katex,
  delimiters: 'dollars',  // $inline$ and $$display$$
});
```

#### 4.3.1 Frontmatter Rendering

YAML frontmatter (`---` delimited at the top of the file) is detected during parsing and rendered as a styled metadata card in preview/read modes.

```typescript
interface FrontmatterBlock extends BlockRange {
  type: 'frontmatter';
  parsed: Record<string, unknown>;  // parsed YAML key-value pairs
}
```

**Preview/Read rendering**: A `<table>` or `<dl>` styled card showing key-value pairs with muted styling:

```html
<div class="frontmatter-card" data-line-start="0" data-line-end="5">
  <div class="frontmatter-label">Frontmatter</div>
  <dl>
    <dt>title</dt><dd>My Document</dd>
    <dt>date</dt><dd>2026-04-15</dd>
    <dt>tags</dt><dd>design, rfc</dd>
  </dl>
</div>
```

**Edit mode**: Shows the raw YAML (including `---` delimiters) in CodeMirror with YAML language support.

#### 4.3.2 Mermaid Diagram Rendering

Fenced code blocks with the `mermaid` language tag are detected and rendered as SVG diagrams in preview/read modes.

```typescript
// During block detection, mermaid blocks are classified:
if (block.type === 'code_block' && block.language === 'mermaid') {
  block.type = 'mermaid';
}
```

**Preview/Read rendering**: The mermaid source is passed to `mermaid.render()` to produce an SVG:

```typescript
import mermaid from 'mermaid';

async function renderMermaidBlock(source: string, id: string): Promise<string> {
  mermaid.initialize({
    startOnLoad: false,
    theme: currentTheme === 'dark' ? 'dark' : 'default',
  });
  const { svg } = await mermaid.render(id, source);
  return `<div class="mermaid-container">${svg}</div>`;
}
```

**Edit mode**: Shows raw mermaid source in CodeMirror. On deactivation, re-renders the SVG.

**Error handling**: If the mermaid source is invalid, show the error message inline below the diagram placeholder instead of crashing.

#### 4.3.3 Math / LaTeX Rendering

Math expressions are rendered using **KaTeX** for fast, high-quality typesetting. Two syntaxes are supported:

- **Inline math**: `$E = mc^2$` renders inline within a paragraph.
- **Display math**: `$$\int_0^\infty e^{-x} dx = 1$$` renders as a centered block.

The `markdown-it-texmath` plugin handles parsing. It injects `math_inline` and `math_block` token types into the markdown-it token stream.

**Block detection**: Display math blocks (`$$...$$`) spanning multiple lines are classified as `math_block` in the block map:

```typescript
// markdown-it-texmath produces math_block tokens with source maps
// These are picked up during block classification:
if (token.type === 'math_block') {
  blocks.push({
    type: 'math_block',
    startLine: token.map[0],
    endLine: token.map[1],
  });
}
```

**Preview/Read rendering**: KaTeX renders the LaTeX source to HTML/MathML:

```typescript
import katex from 'katex';

function renderMath(latex: string, displayMode: boolean): string {
  try {
    return katex.renderToString(latex, {
      displayMode,
      throwOnError: false,   // render error message instead of throwing
      output: 'htmlAndMathml', // accessible output
      trust: false,           // disallow potentially dangerous commands
    });
  } catch {
    return `<span class="math-error">${escapeHtml(latex)}</span>`;
  }
}
```

**Inline math in Live Preview mode**: Inline math (`$...$`) appears within paragraph blocks. When the paragraph is in preview mode, inline math is rendered as KaTeX HTML. When the block enters edit mode, the raw `$...$` delimiters are shown in CodeMirror.

**Display math in Live Preview mode**: A `$$...$$` block renders as a centered KaTeX equation in preview mode. Clicking it activates the block in CodeMirror showing the raw LaTeX source. On deactivation, the equation re-renders.

**Error handling**: Invalid LaTeX renders a styled error message (red text with the raw source) instead of crashing. KaTeX's `throwOnError: false` option handles this.

**Theme integration**: KaTeX output inherits text color from CSS variables. The KaTeX CSS is included in the webview bundle:

```css
/* Math blocks */
.katex-display {
  margin: 1em 0;
  text-align: center;
  color: var(--vscode-editor-foreground);
}

.katex { color: var(--vscode-editor-foreground); }

.math-error {
  color: var(--vscode-errorForeground);
  font-family: var(--vscode-editor-font-family);
  font-style: italic;
}
```

#### 4.3.4 MDX Component Rendering

MDX extends markdown with JSX components. The parser detects MDX constructs (import statements, JSX blocks) and classifies them as `mdx_component` blocks.

**Detection**: MDX components appear as lines starting with `<ComponentName` (capital letter) or `import` / `export` statements. A custom markdown-it rule or a post-parse pass identifies these:

```typescript
// Post-parse pass: identify MDX-specific blocks
function classifyMdxBlocks(blocks: BlockRange[], lines: string[]): void {
  for (const block of blocks) {
    if (block.type === 'html_block') {
      const firstLine = lines[block.startLine].trim();
      // JSX component: starts with <UpperCase
      if (/^<[A-Z]/.test(firstLine)) {
        block.type = 'mdx_component';
      }
    }
    if (block.type === 'paragraph') {
      const firstLine = lines[block.startLine].trim();
      // import/export statements
      if (/^(import|export)\s/.test(firstLine)) {
        block.type = 'mdx_component';
      }
    }
  }
}
```

**Preview/Read rendering**: MDX components are rendered as styled placeholder cards showing the component name and props, since we cannot execute arbitrary JSX in the webview:

```html
<div class="mdx-component-card" data-line-start="10" data-line-end="15">
  <span class="mdx-tag">&lt;Callout type="warning"&gt;</span>
  <div class="mdx-children">...children rendered as markdown...</div>
  <span class="mdx-tag">&lt;/Callout&gt;</span>
</div>
```

For MDX components that wrap markdown content (children), the children are parsed and rendered as normal markdown within the card.

**Edit mode**: Shows the raw MDX/JSX source in CodeMirror with JSX/TSX syntax highlighting.

**`.mdx` file support**: The extension registers for both `*.md` and `*.mdx` files. When the opened file is `.mdx`, the MDX post-parse pass is enabled automatically.

### 4.4 Theme Integration

The webview must match VSCode's current color theme. We achieve this by:

1. Reading CSS variables from the VSCode webview API (`--vscode-editor-background`, `--vscode-editor-foreground`, etc.).
2. Passing the theme kind (`light`, `dark`, `high-contrast`) from the host to the webview.
3. Applying a matching CodeMirror theme (light or dark variant).
4. Using VSCode's CSS variables in the preview CSS so colors adapt automatically.

```css
.live-preview-container {
  background: var(--vscode-editor-background);
  color: var(--vscode-editor-foreground);
  font-family: var(--vscode-editor-font-family);
  font-size: var(--vscode-editor-font-size);
  line-height: var(--vscode-editor-line-height);
}

.live-preview-container h1 {
  color: var(--vscode-textLink-foreground);
  border-bottom: 1px solid var(--vscode-panel-border);
}

.live-preview-container code {
  background: var(--vscode-textCodeBlock-background);
  font-family: var(--vscode-editor-font-family);
}
```

## 5. User Interaction Flow

### 5.1 Opening a File

1. User right-clicks a `.md` / `.mdx` file → "Open With..." → "Markdown Live Preview".
2. Or: User runs command `markdownLivePreview.open` from the command palette.
3. The `CustomTextEditorProvider` creates a webview, sends `doc:init` with the configured default mode.
4. The webview parses the document, builds the block map, and renders using the active mode's renderer.
5. Default mode is configurable (`markdownLivePreview.defaultMode`), defaults to `live-preview`.

### 5.2 Switching Modes

Users can switch modes via:
- **Command palette**: "Markdown Live Preview: Switch to Source/Read/Live Preview Mode"
- **Cycle command**: `markdownLivePreview.cycleMode` (Source → Live Preview → Read → Source)
- **Status bar button**: Shows the current mode (e.g., "MD: Live Preview"), click to cycle.
- **Keyboard shortcut**: `Ctrl+Shift+M` / `Cmd+Shift+M` (configurable).

On mode switch:
1. The current renderer commits any pending edits and tears down.
2. The `ModeController` records the cursor position and scroll offset.
3. The new renderer mounts and restores cursor/scroll position as closely as possible.

### 5.3 Source Mode

Standard text editor behavior. The full document is loaded in a single CodeMirror 6 instance with markdown syntax highlighting. All editing features (multi-cursor, find/replace, etc.) are available. Edits sync to the `TextDocument` via `edit:apply`.

### 5.4 Read Mode

Fully rendered, read-only markdown. The entire document is HTML. Clicking does nothing (no editing). Useful for reviewing a document without accidental edits.

- Links are clickable (open in browser or navigate to header).
- Task list checkboxes are read-only.
- Math expressions render via KaTeX (inline and display).
- Mermaid diagrams render as SVGs.
- Code blocks have syntax highlighting.
- Frontmatter renders as a metadata card.

### 5.5 Live Preview Mode -- Clicking a Block

1. User clicks on a rendered paragraph/heading/etc.
2. The click handler identifies the target block via `data-line-start`.
3. `activateBlock()` is called -- the entire block switches to CodeMirror edit mode.
4. Cursor is placed at the approximate click position (mapped from the click's DOM offset to source line/column).

### 5.6 Live Preview Mode -- Keyboard Navigation Between Blocks

| Key | Behavior |
|-----|----------|
| `ArrowUp` at first line of block | Deactivate current block, activate previous block, cursor at last line |
| `ArrowDown` at last line of block | Deactivate current block, activate next block, cursor at first line |
| `Enter` at end of block | Insert new line in current block (or split block if appropriate) |
| `Backspace` at start of block | Merge with previous block |
| `Escape` | Deactivate current block (return to full preview within Live Preview mode) |
| `Tab` | Indent (inside lists/code blocks) |

### 5.7 Editing Within a Block (Source & Live Preview)

1. User types in the CodeMirror editor.
2. On each change, the new text is debounced (50ms) and sent to the host via `edit:apply`.
3. The host applies a `WorkspaceEdit` to the `TextDocument`.
4. The host sends `edit:ack` with the new document version.
5. In Live Preview mode, other preview blocks remain unchanged (the edit only affects the active block's line range).

### 5.8 External Edits

If another extension or process modifies the file:
1. VSCode fires `onDidChangeTextDocument`.
2. The host sends `doc:update` with the change events to the webview.
3. The document model applies the changes.
4. The active renderer re-renders affected regions:
   - **Source**: CodeMirror applies the diff.
   - **Read**: Re-renders affected block HTML.
   - **Live Preview**: Re-renders affected preview blocks; if the change affects the active edit block, updates CodeMirror with cursor preservation.

## 6. File Structure

```
extensions/markdown-live-preview/
├── package.json
├── tsconfig.json
├── tsdown.config.mts
├── .vscodeignore
├── .gitignore
├── src/                                # Extension host (Node.js)
│   ├── extension.ts                    # Activation, provider & command registration
│   ├── editorProvider.ts               # LivePreviewEditorProvider
│   ├── messages.ts                     # Message type definitions (host ↔ webview)
│   └── util.ts                         # Nonce generation, URI helpers
├── webview/                            # Webview (browser)
│   ├── index.ts                        # Entry point, message dispatch
│   ├── modeController.ts              # Mode switching coordinator
│   ├── documentModel.ts               # Block-indexed document model
│   ├── renderers/
│   │   ├── types.ts                   # Renderer interface
│   │   ├── sourceRenderer.ts          # Full-document CodeMirror editor
│   │   ├── readRenderer.ts            # Read-only HTML renderer
│   │   └── livePreviewRenderer.ts     # Hybrid block preview/edit renderer
│   ├── blockEditor.ts                 # CodeMirror instance factory (shared)
│   ├── keymap.ts                      # Block navigation keybindings
│   ├── plugins/
│   │   ├── frontmatter.ts            # YAML frontmatter parsing & card rendering
│   │   ├── mermaid.ts                # Mermaid diagram rendering
│   │   ├── math.ts                   # KaTeX math rendering (inline + display)
│   │   └── mdx.ts                    # MDX component detection & rendering
│   ├── highlight.ts                   # Syntax highlighting for code blocks
│   └── styles/
│       ├── preview.css                # Preview / read mode styles
│       ├── editor.css                 # CodeMirror overrides
│       ├── frontmatter.css            # Frontmatter card styles
│       ├── math.css                   # KaTeX overrides and error styles
│       └── mermaid.css                # Mermaid container styles
└── test/
    ├── documentModel.test.ts
    ├── modeController.test.ts
    └── plugins/
        ├── frontmatter.test.ts
        ├── math.test.ts
        └── mermaid.test.ts
```

## 7. Build Configuration

The extension requires two build targets:

1. **Extension host code** (`src/`) — Node.js CJS bundle (same as other extensions in this repo).
2. **Webview code** (`webview/`) — Browser ESM bundle (runs in the webview iframe).

```typescript
// tsdown.config.mts
import { defineConfig } from 'tsdown';

export default defineConfig([
  // Extension host bundle
  {
    entry: ['src/extension.ts'],
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    clean: true,
    deps: { neverBundle: ['vscode'] },
  },
  // Webview bundle
  {
    entry: ['webview/index.ts'],
    format: 'esm',
    platform: 'browser',
    outDir: 'dist/webview',
    sourcemap: true,
    // Bundle all dependencies into a single file for the webview
    noExternal: [/.*/],
  },
]);
```

## 8. package.json (Key Fields)

```jsonc
{
  "name": "wx-vsce-markdown-live-preview",
  "displayName": "Markdown Live Preview",
  "description": "Obsidian-style live preview editor for Markdown files with source, read, and live preview modes",
  "version": "0.1.0",
  "private": true,
  "publisher": "weixu",
  "engines": { "vscode": "^1.96.0" },
  "categories": ["Other"],
  "activationEvents": [],
  "main": "./dist/extension.js",
  "contributes": {
    "customEditors": [
      {
        "viewType": "markdownLivePreview.editor",
        "displayName": "Markdown Live Preview",
        "selector": [
          { "filenamePattern": "*.md" },
          { "filenamePattern": "*.markdown" },
          { "filenamePattern": "*.mdx" }
        ],
        "priority": "option"
      }
    ],
    "commands": [
      {
        "command": "markdownLivePreview.open",
        "title": "Open in Live Preview",
        "category": "Markdown Live Preview"
      },
      {
        "command": "markdownLivePreview.cycleMode",
        "title": "Cycle Editor Mode (Source → Live Preview → Read)",
        "category": "Markdown Live Preview"
      },
      {
        "command": "markdownLivePreview.setModeSource",
        "title": "Switch to Source Mode",
        "category": "Markdown Live Preview"
      },
      {
        "command": "markdownLivePreview.setModeRead",
        "title": "Switch to Read Mode",
        "category": "Markdown Live Preview"
      },
      {
        "command": "markdownLivePreview.setModeLivePreview",
        "title": "Switch to Live Preview Mode",
        "category": "Markdown Live Preview"
      }
    ],
    "keybindings": [
      {
        "command": "markdownLivePreview.cycleMode",
        "key": "ctrl+shift+m",
        "mac": "cmd+shift+m",
        "when": "activeCustomEditorId == 'markdownLivePreview.editor'"
      }
    ],
    "configuration": {
      "title": "Markdown Live Preview",
      "properties": {
        "markdownLivePreview.defaultMode": {
          "type": "string",
          "default": "live-preview",
          "enum": ["source", "read", "live-preview"],
          "enumDescriptions": [
            "Raw markdown source editor (CodeMirror)",
            "Read-only rendered preview",
            "Obsidian-style live preview with block-level editing"
          ],
          "description": "Default mode when opening a markdown file"
        },
        "markdownLivePreview.codeBlockTheme": {
          "type": "string",
          "default": "auto",
          "enum": ["auto", "github", "monokai"],
          "description": "Syntax highlighting theme for code blocks"
        },
        "markdownLivePreview.mermaid.enabled": {
          "type": "boolean",
          "default": true,
          "description": "Render mermaid diagrams in preview and read modes"
        },
        "markdownLivePreview.math.enabled": {
          "type": "boolean",
          "default": true,
          "description": "Render LaTeX math expressions ($inline$ and $$display$$) via KaTeX"
        },
        "markdownLivePreview.frontmatter.enabled": {
          "type": "boolean",
          "default": true,
          "description": "Render YAML frontmatter as a styled metadata card"
        }
      }
    }
  },
  "dependencies": {
    "markdown-it": "^14.0.0",
    "markdown-it-front-matter": "^0.2.0",
    "markdown-it-task-lists": "^2.1.0",
    "@codemirror/lang-markdown": "^6.0.0",
    "codemirror": "^6.0.0",
    "@codemirror/theme-one-dark": "^6.0.0",
    "mermaid": "^11.0.0",
    "katex": "^0.16.0",
    "markdown-it-texmath": "^1.0.0",
    "js-yaml": "^4.1.0"
  },
  "devDependencies": {
    "@types/markdown-it": "^14.0.0",
    "@types/katex": "^0.16.0",
    "@types/js-yaml": "^4.0.0",
    "@types/node": "catalog:build",
    "@types/vscode": "^1.96.0",
    "@vscode/vsce": "catalog:build",
    "tsdown": "catalog:build",
    "typescript": "catalog:build",
    "vitest": "catalog:build"
  }
}
```

## 9. Incremental Rendering Strategy

Full re-rendering on every keystroke would be too slow. Instead:

1. **Block-level dirty tracking**: When an edit occurs, only the modified block (and potentially adjacent blocks if line counts change) are re-parsed and re-rendered.
2. **Virtual scrolling** (future): For very large documents, only render blocks visible in the viewport plus a small buffer. Blocks outside the viewport are placeholder `<div>`s with computed heights.
3. **markdown-it caching**: Cache the token stream per block. On edit, only re-tokenize the affected block.

### Re-parse Algorithm

```
on edit(startLine, endLine, newText):
  1. Update lines[] array
  2. Find all blocks overlapping [startLine, endLine]
  3. Expand range to include full blocks (e.g., if edit is in the middle of a list, include the whole list)
  4. Re-tokenize the expanded range with markdown-it
  5. Rebuild blockMap for the affected range
  6. Re-render affected blocks in the DOM
  7. Adjust data-line-start/end attributes for all subsequent blocks (line shift)
```

## 10. Concurrency and Edit Conflicts

Since the `TextDocument` is the source of truth:

- **User edits in webview**: Webview sends `edit:apply` → host applies `WorkspaceEdit` → host sends `edit:ack` with new version → webview updates its version tracking.
- **External edits**: Host receives `onDidChangeTextDocument` → sends `doc:update` to webview → webview applies changes to its model.
- **Conflict**: If the webview sends an `edit:apply` based on an outdated version, the host rejects it and sends the latest `doc:update`. The webview rebases the active editor content.

Version numbers from `TextDocument.version` are used to detect staleness.

## 11. Accessibility

- All preview content uses semantic HTML (`<h1>`–`<h6>`, `<ul>`, `<ol>`, `<table>`, `<blockquote>`, etc.).
- Blocks are `role="button"` with `aria-label="Click to edit"` in preview mode.
- The CodeMirror editor is natively accessible (it supports screen readers and keyboard navigation).
- Focus management: when transitioning between blocks, focus moves to the new CodeMirror instance.

## 12. Known Limitations and Trade-offs

| Concern | Decision |
|---------|----------|
| **No multi-cursor across blocks (Live Preview)** | In Live Preview mode, each block has its own CodeMirror instance. Multi-cursor is supported within a block but not across blocks. Users who need full multi-cursor can switch to Source mode. |
| **Not the default editor** | Registered with `priority: "option"` so it doesn't hijack `.md` files. Users opt in via "Open With..." or by changing the config. |
| **Bundle size** | CodeMirror 6 (~150KB) + mermaid (~250KB) + KaTeX (~120KB CSS+JS) + markdown-it (~30KB). Total ~550KB gzipped. Acceptable since it's loaded once in the webview. Mermaid is lazy-loaded only when a mermaid block is present. KaTeX CSS is always loaded (needed for inline math). |
| **No live collaboration** | The extension works with local files via `TextDocument`. Live collaboration (e.g., Live Share) would require additional work. |
| **Image preview** | Images in preview blocks are shown via `<img>` tags. The `src` must be resolved to a webview URI via `webview.asWebviewUri()`. Relative paths are resolved against the document's directory. |
| **MDX execution** | MDX components are rendered as styled placeholder cards, not executed. True JSX rendering would require a bundler/runtime in the webview. |
| **Mermaid rendering is async** | Large mermaid diagrams may take time to render. A loading spinner is shown while rendering. |

## 13. Resolved Design Decisions

1. **Block granularity** (not line granularity): In Live Preview mode the editing unit is a _block_ -- the entire paragraph, heading, code fence, list, table, etc. enters edit mode when clicked. This avoids splitting multi-line constructs (tables, fenced code blocks, nested lists) at awkward line boundaries and matches the natural unit of markdown structure.

2. **Frontmatter**: Rendered as a styled metadata card (key-value `<dl>` table) in preview/read modes. Raw YAML with `---` delimiters shown in edit mode. Controlled by `markdownLivePreview.frontmatter.enabled`.

3. **Mermaid diagrams**: Supported from the start. Fenced code blocks with `mermaid` language tag render as SVGs. Controlled by `markdownLivePreview.mermaid.enabled`. Mermaid library is lazy-loaded on first use.

4. **MDX support**: `.mdx` files are supported. Import/export statements and JSX components (`<UpperCase>`) are detected and rendered as styled placeholder cards. Markdown children within MDX components are parsed and rendered normally.

5. **Three editor modes**: Source (full CodeMirror), Read (full HTML, no editing), and Live Preview (hybrid). Switchable via command, keybinding, or status bar.

6. **Math / LaTeX**: Supported from the start via KaTeX. Inline math (`$...$`) and display math (`$$...$$`) are rendered in preview/read modes. Uses `markdown-it-texmath` for parsing and KaTeX for rendering. Controlled by `markdownLivePreview.math.enabled`.

## 14. Open Questions

1. **Scroll position preservation**: When switching a block between preview and edit mode, the element height may change, causing scroll jumps. Need to measure the delta and adjust `scrollTop` accordingly. Same concern applies to mode transitions (e.g., Source → Live Preview).

2. **Table editing UX**: Tables are complex multi-line blocks. Should we provide a structured table editor (grid UI) in edit mode, or just show raw pipe-delimited markdown? Initial decision: raw markdown, table grid editor as future enhancement.

3. **Find/replace**: Source mode gets this for free from CodeMirror. Live Preview and Read modes need a custom implementation that searches across blocks. Defer to Phase 4.

## 15. Implementation Phases

### Phase 1: Core Framework & Live Preview
- `CustomTextEditorProvider` with webview scaffolding
- `ModeController` with three renderer stubs
- `DocumentModel` with markdown-it parsing and block-level source maps
- **Live Preview Renderer**: block-level preview rendering, click-to-edit with CodeMirror 6
- Basic keyboard navigation between blocks (ArrowUp/Down, Escape)
- Host ↔ webview edit sync (`edit:apply` / `edit:ack` / `doc:update`)
- Theme integration (light/dark via VSCode CSS variables)

### Phase 2: All Three Modes + Plugins
- **Source Renderer**: full-document CodeMirror editor
- **Read Renderer**: full-document HTML rendering (read-only)
- Mode switching (commands, keybinding, status bar button)
- Cursor and scroll position preservation across mode transitions
- Frontmatter parsing (js-yaml) and card rendering
- Math rendering (KaTeX: `$inline$` and `$$display$$` via markdown-it-texmath)
- Mermaid diagram rendering (lazy-loaded)
- MDX component detection and placeholder rendering
- `.mdx` file support

### Phase 3: Polish
- Scroll position preservation on block edit/preview transitions
- Image preview with `webview.asWebviewUri()` resolution
- Code block syntax highlighting (via Shiki or highlight.js)
- Task list checkbox toggling in preview mode (Read mode stays read-only)
- Link click handling (open in browser or navigate to heading)
- Error handling for invalid mermaid/YAML

### Phase 4: Advanced
- Virtual scrolling for large documents (10K+ lines)
- Find/replace across blocks in Live Preview and Read modes
- Outline/TOC integration (heading navigation)
- Export to PDF/HTML
