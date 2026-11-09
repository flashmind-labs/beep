import { esc, switchHtml } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";

/// Settings, General: one preference for every machine in the mesh.
export function mountSkillSync(host: HTMLElement) {
  mountSyncSetting(host, "skills");
}

export function mountProjectSync(host: HTMLElement) {
  mountSyncSetting(host, "projects");
}

function mountSyncSetting(host: HTMLElement, kind: "skills" | "projects") {
  if (host.dataset.mounted) return;
  host.dataset.mounted = "1";
  host.classList.add("dtoggle");
  let enabled: boolean | null = null;
  let busy = false;
  let error = "";
  const title = kind === "skills" ? "Sync coding agent skills" : "Project sync";
  const get = kind === "skills" ? "skill_sync_get" : "project_sync_get";
  const set = kind === "skills" ? "skill_sync_set" : "project_sync_set";

  const draw = () => {
    const note = kind === "projects" ? enabled === null ? "Loading project sync setting..."
      : enabled ? "Sync projects across all your machines. Turning off keeps existing files in place."
      : "Paused for every project on all machines. Turn on to reconcile your projects."
      : enabled === null ? "Loading skill sync setting..."
      : enabled ? "Keep your coding agent skills in sync across your machines."
      : "Skill syncing is paused on all your machines. Existing files stay in place.";
    host.innerHTML = `<span><b>${title}</b><small>${note}</small>
      ${error ? `<span class="derr" role="alert">${esc(error)}${enabled === null ? ' <button class="dbtn quiet" data-retry>Retry</button>' : ""}</span>` : ""}</span>
      ${switchHtml(enabled ?? false, title, busy || enabled === null ? "disabled" : "")}`;
    host.querySelector(".dswitch")?.addEventListener("click", () => void change());
    host.querySelector("[data-retry]")?.addEventListener("click", () => void load());
  };

  async function load() {
    busy = true;
    error = "";
    draw();

    try {
      enabled = await invoke<boolean>(get);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      draw();
    }
  }

  async function change() {
    if (busy || enabled === null) return;
    const next = !enabled;
    busy = true;
    error = "";
    draw();

    try {
      await invoke(set, { enabled: next });
      enabled = next;
    } catch (e) {
      error = String(e);
      if (kind === "projects") {
        // A partial mesh write can already have applied the master pause.
        try { enabled = await invoke<boolean>(get); } catch { /* Keep the last confirmed value. */ }
      }
    } finally {
      busy = false;
      draw();
    }
  }

  void load();
}
