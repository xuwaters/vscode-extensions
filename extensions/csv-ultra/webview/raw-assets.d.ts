/** Relative `.css` imports are inlined as strings by the `raw-assets` tsdown plugin. */
declare module '*.css' {
  const contents: string;
  export default contents;
}
