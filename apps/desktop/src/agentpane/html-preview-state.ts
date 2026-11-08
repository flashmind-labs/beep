// HTML files offer a rendered preview beside their source.
export function isHtmlFile(path: string): boolean {
  return /\.html?$/i.test(path);
}
