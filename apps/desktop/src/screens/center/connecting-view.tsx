// What Home shows before the machines first answer: pulsing dots, never a blank page.
import { Button } from "../../ui/kit.tsx";
export function ConnectingView({ stuck, text, retry, log }: { stuck: boolean; text: string; retry(): void; log(): void }) {
  const pulse = <div className="cc-pulse"><i /><i /><i /></div>;
  if (!stuck) return <div className="cc-conn">{pulse}<b>Connecting to your machines…</b></div>;
  return <div className="cc-conn stuck">{pulse}<b>Still connecting to your machines</b><span className="why">{text}</span><div className="acts"><Button look="primary" onClick={retry}>Retry now</Button><Button onClick={log}>Show log</Button></div></div>;
}
