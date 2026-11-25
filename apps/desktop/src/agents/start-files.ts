import { uploadParts, withMentions } from "../agentpane/files-state.ts";
import { fileBase64 } from "../chat/pics.ts";
import { invoke } from "../ui/tauri.ts";

/// The machine holding the cache, and the work folder older nodes check.
interface Destination { machine: string; root: string }

export interface PendingFile {
  file: File;
  /// When its upload started.
  started: number;
  /// Where the file is going, and its path there once complete.
  at: Destination | null;
  path: string | null;
  done: number;
  error: string | null;
}

const absolute = (root: string) => /^(?:\/|[A-Za-z]:[\\/])/.test(root);

// Draft files upload to the selected machine's cache. A file removed,
// stashed or left behind is deleted there again.
export class StartFiles {
  readonly pending: PendingFile[] = [];
  private readonly draft = crypto.randomUUID();
  private where: Destination | null = null;
  private running: Promise<void> | null = null;

  private readonly changed: () => void;
  private readonly failed: (message: string) => void;

  constructor(changed = () => {}, failed = (_message: string) => {}) {
    this.changed = changed;
    this.failed = failed;
  }

  add(files: Iterable<File>) {
    for (const file of files) this.pending.push({ file, started: Date.now(), at: null, path: null, done: 0, error: null });
    this.changed();
    void this.pump();
  }

  /// Follows the form's machine. Folders on that machine share its cache.
  moveTo(machine: string, root: string) {
    root = absolute(root) ? root : "";
    if (this.where?.machine === machine) {
      this.where.root = root;
      return;
    }
    if (!this.where && !machine) return;
    this.where = machine ? { machine, root } : null;
    for (const entry of this.pending) entry.error = null;
    void this.pump();
  }

  remove(index: number) {
    const [entry] = this.pending.splice(index, 1);
    if (entry) this.discard(entry);
    this.changed();
  }

  /// Takes every file off and deletes what was uploaded.
  discardAll() {
    for (const entry of this.pending.splice(0)) this.discard(entry);
    this.changed();
  }

  /// Takes every file off and keeps what was uploaded: the agent reads it.
  forget() {
    this.pending.length = 0;
    this.changed();
  }

  /// The bytes sent of the files still on their way, or null when none is.
  get progress() {
    if (!this.pending.some(entry => entry.at && !entry.path && !entry.error)) return null;
    const live = this.pending.filter(entry => !entry.error);

    return { size: live.reduce((n, e) => n + e.file.size, 0), done: live.reduce((n, e) => n + (e.path ? e.file.size : e.done), 0) };
  }

  async prompt(text: string, machine: string, root: string) {
    if (this.pending.length && !machine) throw new Error("Pick a machine before attaching files");
    this.moveTo(machine, root);
    for (const entry of this.pending) entry.error = null;
    while (this.next() || this.running) await (this.running ?? this.pump());
    const failed = this.pending.find(entry => entry.error);
    if (failed?.error) throw new Error(failed.error);

    return withMentions(text, this.pending.map(({ path }) => {
      if (!path) throw new Error("An attachment did not finish uploading");
      return path;
    }));
  }

  private next() {
    const where = this.where;
    if (!where) return null;
    const entry = this.pending.find(e => !e.error && !(e.path && e.at === where));

    return entry ? { entry, where } : null;
  }

  private pump(): Promise<void> {
    this.running ??= (async () => {
      for (let next = this.next(); next; next = this.next()) await this.send(next.entry, next.where);
    })().finally(() => {
      this.running = null;
      if (this.next()) void this.pump();
    });

    return this.running;
  }

  private async send(entry: PendingFile, where: Destination) {
    if (entry.path) this.discard(entry);
    Object.assign(entry, { at: where, started: Date.now(), path: null, done: 0, error: null });
    this.changed();
    const name = entry.file.name || "file";
    let path = name, written = false;
    const gone = () => !this.pending.includes(entry) || this.where !== where;

    try {
      for (const part of uploadParts(entry.file.size)) {
        if (gone()) break;
        let data: string;

        try { data = await fileBase64(entry.file.slice(part.offset, part.offset + part.len)); }
        catch { throw new Error(`${name} could not be read; drop files rather than folders`); }

        path = await invoke<string>("agent_tree_upload", {
          machineId: where.machine, root: where.root, path, offset: part.offset, size: entry.file.size, data,
          attachment: { agent: "draft", session: this.draft },
        });
        written = true;
        entry.done = part.offset + part.len;
        this.changed();
      }

      if (gone()) {
        if (written) this.removeUpload(where.machine, path);
        if (entry.at === where) entry.at = null;
      } else entry.path = path;
    } catch (e) {
      if (written) this.removeUpload(where.machine, path);
      if (!gone()) {
        entry.error = String(e instanceof Error ? e.message : e).replace(/^bad request: /, "");
        this.failed(entry.error);
      }
    }

    this.changed();
  }

  private discard(entry: PendingFile) {
    if (entry.at && entry.path) this.removeUpload(entry.at.machine, entry.path);
    entry.at = null;
    entry.path = null;
  }
  private removeUpload(machineId: string, path: string) {
    invoke("agent_discard_draft_attachment", { machineId, draft: this.draft, path })
      .catch(e => console.warn("could not discard a cached upload", e));
  }
}
