import type { CSSProperties } from "react";
import { look } from "./research-state.ts";

export function Avatar({ id }: { id: string }) {
  const avatar = look(id);
  const style: CSSProperties & { "--c": string } = { "--c": avatar.c };
  return <span className="rav" style={style}>{avatar.ini}</span>;
}
