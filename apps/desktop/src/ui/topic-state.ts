// Dotted topic names. A trailing wildcard covers the remaining resource scope.

export type Topic = string;

interface Parsed {
  name: string;
  scope: string[];
  wildcard: boolean;
  canonical: Topic;
}

const utf8 = new TextEncoder();
const encode = (part: string) => encodeURIComponent(part).replace(/[.!*'()]/g, c => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);

function parse(value: Topic): Parsed | null {
  if (value === "*") return { name: "*", scope: [], wildcard: true, canonical: "*" };
  if (!value.length || value.length > 16_384) return null;
  const parts = value.split(".");
  const wildcard = parts.length === 1 || parts.at(-1) === "*";
  if (parts.at(-1) === "*") parts.pop();
  let decoded: string[];

  try {
    if (parts.some(part => !/^[A-Za-z0-9_~%\-]+$/.test(part))) return null;
    decoded = parts.map(part => decodeURIComponent(part));
  } catch { return null; }

  const [name, ...scope] = decoded;
  if (!name || name.length > 128 || !/^[A-Za-z0-9._/\-]+$/.test(name)
    || scope.some(part => !part.length || part.includes("\0")) || utf8.encode(scope.join(".")).length > 4096) return null;
  return { name, scope, wildcard, canonical: [name, ...scope].map(encode).join(".") + (wildcard ? ".*" : "") };
}

export function normalizeTopic(value: Topic): Topic {
  const parsed = parse(value);
  if (!parsed) throw new Error("Invalid topic: use literal segments and an optional trailing wildcard");
  return parsed.canonical;
}

/// Existing resource keys are one literal segment, even when they contain dots or slashes.
export function topic(name: string, key: string | null = null): Topic {
  if (name === "*" && key === null) return "*";
  return normalizeTopic(`${encode(name)}.${key === null ? "*" : encode(key)}`);
}

export function matchesTopic(wanted: Topic, heard: Topic): boolean {
  const left = parse(wanted), right = parse(heard);
  if (!left || !right) return false;
  if (left.name === "*" || right.name === "*") return true;
  if (left.name !== right.name) return false;
  const count = Math.min(left.scope.length, right.scope.length);
  for (let i = 0; i < count; i++) if (left.scope[i] !== right.scope[i]) return false;
  return left.scope.length === right.scope.length
    || left.wildcard && left.scope.length < right.scope.length
    || right.wildcard && right.scope.length < left.scope.length;
}
