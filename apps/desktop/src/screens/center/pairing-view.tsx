import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { Button, NO_AUTO } from "../../ui/kit.tsx";
import { reason } from "../../ui/format.ts";

export interface PairingCommands {
  checkPermission(): Promise<{ camera: string }>;
  requestPermission(): Promise<{ camera: string }>;
  scan(): Promise<{ content?: string }>;
  cancelScan(): Promise<unknown>;
  pair(ticket: string): Promise<unknown>;
}

type Phase = "idle" | "permission" | "scanning" | "pairing";

export function PairingScreen({ commands, done }: { commands: PairingCommands; done: () => void }) {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const active = useRef(true);
  const running = useRef(false);
  const cameraOpen = useRef(false);

  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      if (cameraOpen.current) void commands.cancelScan().catch(() => {});
    };
  }, [commands]);

  async function pair(ticket: string) {
    setPhase("pairing");
    setError(null);

    try {
      await commands.pair(ticket);
      if (active.current) done();
    } catch (e) {
      if (active.current) { setError(reason(e)); setPhase("idle"); }
    }
  }

  async function submit() {
    const ticket = code.trim();
    if (!ticket || running.current) return;
    running.current = true;
    try { await pair(ticket); } finally { running.current = false; }
  }

  async function scan() {
    if (running.current) return;
    running.current = true;
    setError(null);
    setPhase("permission");
    let scanning = false;

    try {
      let permission = (await commands.checkPermission()).camera;
      if (!active.current) return;
      if (permission !== "granted") permission = (await commands.requestPermission()).camera;
      if (!active.current) return;
      if (permission !== "granted") { setError("Camera access is off. Allow it in Settings, or paste the code."); setPhase("idle"); return; }
      scanning = true;
      flushSync(() => setPhase("scanning"));
      cameraOpen.current = true;
      const found = await commands.scan();
      cameraOpen.current = false;
      if (!active.current) return;
      const ticket = found.content?.trim();
      if (!ticket) { setError("Could not read that code."); setPhase("idle"); return; }
      setCode(ticket);
      await pair(ticket);
    } catch (e) {
      if (active.current) { const said = reason(e); setError(scanning && said === "cancelled" ? "Scan cancelled." : said); setPhase("idle"); }
    } finally {
      cameraOpen.current = false;
      running.current = false;
    }
  }

  return <main className={phase === "scanning" ? "pair scanning" : "pair"}>
    <div className="pair-body">
      <h1>Pair this phone</h1>
      <p className="pair-sub">On a desktop, open Add machine and pick phone.</p>
      <Button look="primary" disabled={phase !== "idle"} onClick={() => void scan()}>Scan QR code</Button>
      <textarea className="pair-code" placeholder="Or paste the pairing code" {...NO_AUTO} disabled={phase !== "idle"} value={code} onChange={e => setCode(e.currentTarget.value)} />
      <Button disabled={phase !== "idle" || !code.trim()} onClick={() => void submit()}>{phase === "pairing" ? "Pairing" : "Pair"}</Button>
      <p className="pair-error" role="alert" hidden={error === null}>{error}</p>
    </div>
    {phase === "scanning" && <div className="pair-scan"><p>Point the camera at the code</p><Button onClick={() => { void commands.cancelScan().catch(() => {}); }}>Cancel</Button></div>}
  </main>;
}
