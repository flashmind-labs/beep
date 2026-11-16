// SVG files are bundled as their text (build.mjs).
declare module "*.svg" {
  const text: string;
  export default text;
}
