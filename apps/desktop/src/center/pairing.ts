// A phone with no mesh yet: paste the code from a desktop's Add machine sheet.
import { button, el } from "../ui/dom.ts";
import { onPhone } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";

function message(e: unknown) {
  const said = e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? e.message : e;
  return String(said).replace(/^bad request: /, "");
}

/// Resolves once this phone belongs to a mesh. A desktop needs no pairing; an unpaired phone
/// shows the pairing screen over the page until a code works.
export async function pairing(): Promise<void> {
  if (!onPhone()) return;
  if (await invoke<boolean>("mobile_paired").catch(() => false)) return;
  const field = el("textarea", "pair-code");
  field.placeholder = "Pairing code";
  field.setAttribute("autocapitalize", "off");
  field.setAttribute("autocomplete", "off");
  field.spellcheck = false;
  const error = el("p", "pair-error");
  error.setAttribute("role", "alert");
  error.hidden = true;
  const root = el("main", "pair");
  await new Promise<void>(done => {
    const go = button("Pair", "dbtn pri", async () => {
      const ticket = field.value.trim();
      if (!ticket) return;
      go.disabled = field.disabled = true;
      go.textContent = "Pairing";
      error.hidden = true;
      try {
        await invoke("mobile_pair", { ticket });
        root.remove();
        done();
      } catch (e) {
        error.textContent = message(e);
        error.hidden = false;
        go.disabled = field.disabled = false;
        go.textContent = "Pair";
      }
    });
    go.disabled = true;
    field.addEventListener("input", () => { go.disabled = !field.value.trim(); });
    root.append(
      el("div", "pair-body",
        el("h1", "", "Pair this phone"),
        el("p", "pair-sub", "On a desktop, open Add machine and pick phone."),
        field,
        error),
      el("div", "pair-actions", go));
    document.body.append(root);
    field.focus();
  });
}
