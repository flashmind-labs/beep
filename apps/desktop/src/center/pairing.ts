// A phone with no mesh yet: scan or paste the code from a desktop's Add machine sheet.
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
  field.placeholder = "Or paste the pairing code";
  field.setAttribute("autocapitalize", "off");
  field.setAttribute("autocomplete", "off");
  field.spellcheck = false;
  const error = el("p", "pair-error");
  error.setAttribute("role", "alert");
  error.hidden = true;
  const root = el("main", "pair");
  await new Promise<void>(done => {
    const busy = (on: boolean) => {
      scan.disabled = field.disabled = on;
      go.disabled = on || !field.value.trim();
      go.textContent = on ? "Pairing" : "Pair";
    };
    const pair = async (ticket: string) => {
      busy(true);
      error.hidden = true;
      try {
        await invoke("mobile_pair", { ticket });
        root.remove();
        done();
      } catch (e) {
        fail(message(e));
        busy(false);
      }
    };
    const fail = (text: string) => {
      error.textContent = text;
      error.hidden = false;
    };
    const go = button("Pair", "dbtn", () => {
      const ticket = field.value.trim();
      if (ticket) void pair(ticket);
    });
    go.disabled = true;
    const scan = button("Scan QR code", "dbtn pri", async () => {
      error.hidden = true;
      const code = await scanCode(root, fail);
      if (!code) return;
      field.value = code;
      await pair(code);
    });
    field.addEventListener("input", () => { go.disabled = !field.value.trim(); });
    root.append(
      el("div", "pair-body",
        el("h1", "", "Pair this phone"),
        el("p", "pair-sub", "On a desktop, open Add machine and pick phone."),
        scan,
        field,
        go,
        error));
    document.body.append(root);
  });
}

const SCANNER = "plugin:barcode-scanner|";

/// Opens the camera over the screen and resolves with the QR text, or null after saying why
/// through `fail` (camera blocked, cancelled, unreadable).
async function scanCode(root: HTMLElement, fail: (text: string) => void): Promise<string | null> {
  try {
    let state = (await invoke<{ camera: string }>(SCANNER + "check_permissions")).camera;
    if (state !== "granted") state = (await invoke<{ camera: string }>(SCANNER + "request_permissions")).camera;
    if (state !== "granted") {
      fail("Camera access is off. Allow it in Settings, or paste the code.");
      return null;
    }
  } catch (e) {
    fail(message(e));
    return null;
  }
  // The camera shows behind a transparent page: only the cancel bar stays drawn.
  const stop = button("Cancel", "dbtn", () => { void invoke(SCANNER + "cancel").catch(() => {}); });
  const bar = el("div", "pair-scan", el("p", "", "Point the camera at the code"), stop);
  root.classList.add("scanning");
  root.append(bar);
  try {
    const found = await invoke<{ content?: string }>(SCANNER + "scan", { formats: ["QR_CODE"], windowed: false });
    const text = found.content?.trim();
    if (!text) fail("Could not read that code.");
    return text || null;
  } catch (e) {
    const said = message(e);
    fail(said === "cancelled" ? "Scan cancelled." : said);
    return null;
  } finally {
    root.classList.remove("scanning");
    bar.remove();
  }
}
