import { useEffect, useRef, useState, type CSSProperties } from "react";
import { type AppLogo as Logo, logoFor } from "./approvals.ts";
import { invoke } from "../ui/tauri.ts";

export function AppLogo({ app, style, onReady }: { app: string; style?: CSSProperties; onReady?: () => void }) {
  const [result, setResult] = useState<{ app: string; logo: string | null; failed: boolean } | null>(null);
  const ready = useRef(onReady);
  ready.current = onReady;
  useEffect(() => {
    let active = true;
    void loadAppLogo(app).then(logo => { if (active) { setResult({ app, logo, failed: false }); ready.current?.(); } });
    return () => { active = false; };
  }, [app]);
  const shown = result?.app === app ? result : null;
  const logo = shown && !shown.failed ? shown.logo : null;
  return <span className={`dapp${logo ? " logo" : ""}`} data-app={app} style={style}>{logo ? <img src={logo} alt="" onError={() => setResult(current => current?.app === app ? { app, logo, failed: true } : current)} /> : shown ? (app || "?")[0].toUpperCase() : null}</span>;
}

// A failed list is retried when another logo mounts.
let apps: Promise<Logo[]> | null = null;
async function loadAppLogo(app: string) {
  const list = await (apps ??= invoke<Logo[]>("list_apps").catch(() => { apps = null; return []; }));
  return logoFor(list, app);
}
