// A phone with no mesh yet: scan or paste the code from a desktop's Add machine sheet.
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { onPhone } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";
import { PairingScreen } from "./pairing-view.tsx";

const SCANNER = "plugin:barcode-scanner|";

/// Resolves once this phone belongs to a mesh. A desktop needs no pairing; an unpaired phone
/// shows the pairing screen over the page until a code works.
export async function pairing(): Promise<void> {
  if (!onPhone()) return;
  if (await invoke<boolean>("mobile_paired").catch(() => false)) return;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);

  await new Promise<void>(done => {
    root.render(createElement(PairingScreen, {
      commands: {
        checkPermission: () => invoke<{ camera: string }>(SCANNER + "check_permissions"),
        requestPermission: () => invoke<{ camera: string }>(SCANNER + "request_permissions"),
        scan: () => invoke<{ content?: string }>(SCANNER + "scan", { formats: ["QR_CODE"], windowed: false }),
        cancelScan: () => invoke(SCANNER + "cancel"),
        pair: ticket => invoke("mobile_pair", { ticket }),
      },
      done: () => { root.unmount(); host.remove(); done(); },
    }));
  });
}
