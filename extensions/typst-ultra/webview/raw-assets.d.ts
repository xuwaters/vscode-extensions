/**
 * `.css` imported from the webview is inlined as a string by the `raw-assets`
 * plugin in `tsdown.config.mts`, so the stylesheet stays a real file on disk
 * with real editor support and still ships inside one bundle.
 */
declare module '*.css' {
  const contents: string;
  export default contents;
}
