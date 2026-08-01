// CSS files are imported for their side effect: tsdown concatenates them into
// the emitted `dist/webview/style.css` (and we copy referenced font assets).
declare module '*.css';

// markdown-it plugins that ship without bundled type declarations.
declare module 'markdown-it-footnote';
declare module 'markdown-it-task-lists';
declare module 'markdown-it-texmath';
