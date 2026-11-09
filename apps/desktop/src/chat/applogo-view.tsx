import { useEffect, useRef, useState, type CSSProperties } from "react";
import { loadAppLogo } from "./applogo.ts";

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
