// HTML and Markdown files offer a rendered preview beside their source.
export function isHtmlFile(path: string): boolean {
  return /\.html?$/i.test(path);
}

function isMarkdownFile(path: string): boolean {
  return /\.(md|markdown)$/i.test(path);
}

export function hasPreview(path: string): boolean {
  return isHtmlFile(path) || isMarkdownFile(path);
}
