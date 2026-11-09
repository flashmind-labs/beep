import { type AppLogo, logoFor } from "./approvals.ts";
import { invoke } from "../ui/tauri.ts";

// A failed list is retried when another logo mounts.
let apps: Promise<AppLogo[]> | null = null;
export async function loadAppLogo(app: string) {
  const list = await (apps ??= invoke<AppLogo[]>("list_apps").catch(() => { apps = null; return []; }));
  return logoFor(list, app);
}
