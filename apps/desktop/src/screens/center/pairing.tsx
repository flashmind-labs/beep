// A phone with no mesh yet: scan or paste the code from a desktop's Add machine sheet.
import { createDocumentScope } from "../../ui/document-root.tsx";
import { APPLE, onPhone } from "../../ui/platform.ts";
import { invoke } from "../../ui/tauri.ts";
import { PairingScreen, type PairingCommands } from "./pairing-view.tsx";

const SCANNER = "plugin:barcode-scanner|";
const GRANTED = { camera: "granted" };

type Camera = Omit<PairingCommands, "pair">;

/// Tauri's barcode scanner, drawn under the page.
const IOS_CAMERA: Camera = {
  checkPermission: () => invoke<{ camera: string }>(SCANNER + "check_permissions"),
  requestPermission: () => invoke<{ camera: string }>(SCANNER + "request_permissions"),
  scan: () => invoke<{ content?: string }>(SCANNER + "scan", { formats: ["QR_CODE"], windowed: false }),
  cancelScan: () => invoke(SCANNER + "cancel"),
};

/// zxing's capture screen, a screen of its own that asks for the camera and closes on Back.
const ANDROID_CAMERA: Camera = {
  checkPermission: () => Promise.resolve(GRANTED),
  requestPermission: () => Promise.resolve(GRANTED),
  scan: () => invoke<{ content?: string }>("plugin:qr-scan|scan"),
  cancelScan: () => Promise.resolve(),
};

/// Resolves once this phone belongs to a mesh. A desktop needs no pairing; an unpaired phone
/// shows the pairing screen over the page until a code works.
export async function pairing(): Promise<void> {
  if (!onPhone()) return;
  if (await invoke<boolean>("mobile_paired").catch(() => false)) return;
  const screen = createDocumentScope<"pairing">();

  await new Promise<void>(done => {
    screen.set([{ slot: "pairing", host: document.body, content: <div><PairingScreen
      commands={{ ...(APPLE ? IOS_CAMERA : ANDROID_CAMERA), pair: ticket => invoke("mobile_pair", { ticket }) }}
      done={() => { screen.dispose(); done(); }}
    /></div> }]);
  });
}
