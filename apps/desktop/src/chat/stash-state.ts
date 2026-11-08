export interface StashFile {
  name: string;
  mime: string;
  size: number;
  data: string;
}

export interface StashMessage {
  text: string;
  images: StashFile[];
  files: StashFile[];
  paths: string[];
}

export interface StashEntry {
  id: string;
  text: string;
  images: string[];
  files: string[];
}

export function hasStashMessage(message: StashMessage) {
  return !!message.text.trim() || !!message.images.length || !!message.files.length || !!message.paths.length;
}

export function stashFile(file: StashFile) {
  const bytes = Uint8Array.from(atob(file.data), char => char.charCodeAt(0));
  if (bytes.length !== file.size) throw new Error(`${file.name} has incomplete attachment data`);
  return new File([bytes], file.name, { type: file.mime });
}
