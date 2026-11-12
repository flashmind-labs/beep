import { joinPath, uploadParts, uploadRefusal, withMentions } from "../agentpane/files-state.ts";
import { fileBase64 } from "../chat/attach.ts";
import { invoke } from "../ui/tauri.ts";

interface PendingFile {
  file: File;
  uploaded?: { machine: string; root: string; path: string };
}

// Files wait locally until Start fixes their destination. Keep completed uploads
// for retries, and use absolute paths so a new worktree can read them too.
export class StartFiles {
  readonly pending: PendingFile[] = [];

  add(files: Iterable<File>, error: (text: string) => void) {
    for (const file of files) {
      const refusal = uploadRefusal(file);
      if (refusal) error(refusal);
      else this.pending.push({ file });
    }
  }

  async prompt(text: string, machine: string, root: string, progress: (text: string) => void) {
    if (this.pending.length && !/^(?:\/|[A-Za-z]:[\\/])/.test(root)) throw new Error("Pick a project folder before attaching files");
    const paths: string[] = [];

    for (const entry of this.pending) {
      const { file } = entry;

      if (entry.uploaded?.machine !== machine || entry.uploaded.root !== root) {
        const name = file.name || "file";
        let path = joinPath(".beep/uploads", name);
        progress(`Uploading ${name}: 0%`);

        for (const part of uploadParts(file.size)) {
          let data: string;

          try { data = await fileBase64(file.slice(part.offset, part.offset + part.len)); }
          catch { throw new Error(`${file.name || "That file"} could not be read; drop files rather than folders`); }

          path = await invoke<string>("agent_tree_upload", { machineId: machine, root, path, offset: part.offset, size: file.size, data, attachment: null });
          progress(`Uploading ${name}: ${file.size ? Math.round((part.offset + part.len) / file.size * 100) : 100}%`);
        }

        entry.uploaded = { machine, root, path: joinPath(root, path) };
      }

      paths.push(entry.uploaded.path);
    }

    return withMentions(text, paths);
  }
}
