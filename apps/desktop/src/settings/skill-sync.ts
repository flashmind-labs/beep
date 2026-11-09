import { esc, switchHtml } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";

/// Settings, General: one preference for every machine in the mesh.
export function mountSkillSync(host: HTMLElement) {
  if (host.dataset.mounted) return;
  host.dataset.mounted = "1";
  host.classList.add("dtoggle");
  let enabled: boolean | null = null;
  let busy = false;
  let error = "";

  const draw = () => {
    const note = enabled === null ? "Loading skill sync setting..."
      : enabled ? "Keep your coding agent skills in sync across your machines."
      : "Skill syncing is paused on all your machines. Existing files stay in place.";
    host.innerHTML = `<span><b>Sync coding agent skills</b><small>${note}</small>
      ${error ? `<span class="derr" role="alert">${esc(error)}${enabled === null ? ' <button class="dbtn quiet" data-retry>Retry</button>' : ""}</span>` : ""}</span>
      ${switchHtml(enabled ?? false, "Sync coding agent skills", busy || enabled === null ? "disabled" : "")}`;
    host.querySelector(".dswitch")?.addEventListener("click", () => void change());
    host.querySelector("[data-retry]")?.addEventListener("click", () => void load());
  };

  async function load() {
    busy = true;
    error = "";
    draw();

    try {
      enabled = await invoke<boolean>("skill_sync_get");
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
      await invoke("skill_sync_set", { enabled: next });
      enabled = next;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      draw();
    }
  }

  void load();
}
