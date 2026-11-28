import { useEffect, useState, type FormEvent } from "react";
import { Button, Checkbox, EmptyPanel, LineRow, NO_AUTO, Picker, SectionHead, SheetHead, SkeletonRows } from "../../ui/kit.tsx";
import { confirmSheet, openMenu } from "../../ui/menu.tsx";
import { openSheet } from "../../ui/parts.tsx";
import { HarnessIcon, Icon } from "../../ui/icons.tsx";
import type { invoke as Invoke } from "../../ui/tauri.ts";
import { reason } from "../../ui/format.ts";

type Harness = "codex" | "claude" | "agy";
const HARNESS = { codex: "Codex", claude: "Claude Code", agy: "Antigravity" } satisfies Record<Harness, string>;
const TRANSPORT = { stdio: "Stdio", http: "Streamable HTTP", sse: "SSE", ws: "WebSocket" };
type Transport = keyof typeof TRANSPORT;
type RemoteTransport = Exclude<Transport, "stdio">;
interface Stdio {
  command: string; args: string[]; env: Record<string, string>; cwd: string | null;
}
interface Remote {
  transport: RemoteTransport; url: string; headers: Record<string, string>;
  env_http_headers: Record<string, string>; bearer_token_env_var: string | null;
}
const APPROVAL = { auto: "Auto", prompt: "Prompt", writes: "Writes", approve: "Approve" };
type Approval = keyof typeof APPROVAL;
interface CodexOptions {
  required: boolean | null;
  startup_timeout_sec: number | null;
  tool_timeout_sec: number | null;
  enabled_tools: string[] | null;
  disabled_tools: string[] | null;
  default_tools_approval_mode: Approval | null;
}
interface ClaudeOauth {
  clientId: string | null;
  callbackPort: number | null;
  authServerMetadataUrl: string | null;
  scopes: string | null;
}
interface Settings {
  connection: { stdio: Stdio } | { remote: Remote };
  enabled: boolean;
  codex: CodexOptions | null;
  claude_oauth: ClaudeOauth | null;
}
interface Server { name: string; settings: Settings | null }
interface EditableServer { name: string; settings: Settings }
type SaveStatus = "saved" | "conflict";
interface SaveResult { status: SaveStatus; servers: Server[] }
interface Applied { name: string; error: string | null }
type Load<T> = { step: "loading" } | { step: "ready"; value: T } | { step: "failed"; error: string };
interface MachineProps { machineId: string; invoke: typeof Invoke }

export function MachineMcpServers({ machineId, online, invoke }: MachineProps & { online: boolean }) {
  return <section className="mcp-servers" aria-label="MCP servers">
    {online ? <Harnesses machineId={machineId} invoke={invoke} />
      : <><SectionHead title="MCP servers" /><p className="dlead">Connect this machine to manage its MCP servers.</p></>}
  </section>;
}

function Harnesses({ machineId, invoke }: MachineProps) {
  const [load, setLoad] = useState<Load<Harness[]>>({ step: "loading" });
  const [selected, setSelected] = useState<Harness | null>(null);
  const [retry, setRetry] = useState(0);

  useEffect(() => {
    let current = true;
    void invoke<Harness[]>("machine_mcp_harnesses", { machineId }).then(
      value => { if (current) setLoad({ step: "ready", value }); },
      error => { if (current) setLoad({ step: "failed", error: reason(error) }); },
    );
    return () => { current = false; };
  }, [machineId, invoke, retry]);

  if (load.step === "loading") return <><SectionHead title="MCP servers" /><SkeletonRows widths={[72, 49]} /></>;
  if (load.step === "failed") return <><SectionHead title="MCP servers" /><ReadError error={load.error} retry={() => { setLoad({ step: "loading" }); setRetry(retry + 1); }} /></>;

  const harness = selected ?? load.value[0] ?? null;
  if (!harness) return <><SectionHead title="MCP servers" /><p className="dlead">Install Codex or Claude Code on this machine to manage its MCP servers.</p></>;

  const options = load.value.map(id => ({ id, label: HARNESS[id] }));
  return <>
    <SectionHead title="MCP servers">
      <Picker<Harness> label="Harness" value={harness} options={options} onChange={setSelected} icon={<HarnessIcon agent={harness} />} />
    </SectionHead>
    <McpServers key={harness} machineId={machineId} harness={harness} invoke={invoke} />
  </>;
}

function McpServers({ machineId, harness, invoke }: MachineProps & { harness: Harness }) {
  const [load, setLoad] = useState<Load<Server[]>>({ step: "loading" });
  const [retry, setRetry] = useState(0);
  const [applied, setApplied] = useState<Applied[] | null>(null);
  const title = HARNESS[harness];

  useEffect(() => {
    let current = true;
    void invoke<Server[]>("machine_mcp_servers", { machineId, harness }).then(
      value => { if (current) setLoad({ step: "ready", value }); },
      error => { if (current) setLoad({ step: "failed", error: reason(error) }); },
    );
    return () => { current = false; };
  }, [machineId, harness, retry, invoke]);

  function edit(server: Server | null) {
    setApplied(null);
    const previous = server?.settings ? { name: server.name, settings: server.settings } : null;
    openEditor(harness, server, async update => {
      const result = await invoke<SaveResult>("save_machine_mcp_server", { machineId, harness, previous, server: update });
      setLoad({ step: "ready", value: result.servers });
      return result.status;
    });
  }

  function menu(event: MouseEvent, server: Server) {
    const { name } = server;
    const args = { machineId, harness, name };
    openMenu(event, name, [
      { label: "Edit", on: () => edit(server) },
      { label: "Apply to all", on: () => confirmSheet({
        title: `Apply ${name} to all machines?`,
        text: `Copies this entry, including any credentials in it, to ${title} on other linked machines. A server with the same name is replaced.`,
        action: "Apply to all machines",
        run: async () => { setApplied(await invoke<Applied[]>("apply_machine_mcp_server", args)); },
      }) },
      { kind: "separator" },
      { label: "Remove", warn: true, on: () => confirmSheet({
        title: `Remove ${name} from ${title}?`,
        text: `Removes this server from ${title}’s user configuration on this machine.`,
        action: "Remove server",
        run: async () => {
          const value = await invoke<Server[]>("remove_machine_mcp_server", args);
          setLoad({ step: "ready", value });
          setApplied(null);
        },
      }) },
    ]);
  }

  if (load.step === "loading") return <SkeletonRows widths={[72, 49]} />;
  if (load.step === "failed") return <ReadError error={load.error} retry={() => { setLoad({ step: "loading" }); setRetry(retry + 1); }} />;

  return <>
    {load.value.length === 0 ? <EmptyPanel title="No user servers configured" /> : <div className="cc-panel">
      {load.value.map(server => <LineRow key={server.name} lead={<Icon name="plugin" size={16} />} title={server.name} sub={serverDetail(server.settings)} onOpen={() => edit(server)}
        onContext={event => menu(event, server)}
        right={<Button look="quiet" small aria-label={`More for ${server.name}`} aria-haspopup="menu" onClick={event => {
          event.stopPropagation();
          menu(event.nativeEvent, server);
        }}>&#8943;</Button>} />)}
    </div>}
    <div className="dline dmt6"><Button small onClick={() => edit(null)}>+ Add server</Button></div>
    {applied && <ApplyResults results={applied} />}
  </>;
}

function ReadError({ error, retry }: { error: string; retry: () => void }) {
  return <div className="dline wrap">
    <span className="derr" role="alert">{error}</span>
    <Button small onClick={retry}>Try again</Button>
  </div>;
}

function ApplyResults({ results }: { results: Applied[] }) {
  const failures = results.filter(result => result.error !== null);
  const count = results.length - failures.length;

  return <div role="status" className="dlead">
    {results.length === 0 ? "There are no other linked agent hosts." : `Applied to ${count} of ${results.length} other machines.`}
    {failures.length > 0 && <ul>{failures.map((failure, index) => <li key={index}><b>{failure.name}</b> · {failure.error}</li>)}</ul>}
  </div>;
}

function openEditor(harness: Harness, server: Server | null, save: (server: EditableServer) => Promise<SaveStatus>) {
  const title = server ? `Edit ${server.name}` : "Add MCP server";
  let busy = false;
  const sheet = openSheet(title, "am-sheet", { canClose: () => !busy });
  if (server?.settings === null) {
    sheet.render(<><SheetHead title={title} /><div className="am-msg">This server uses a configuration Beep can’t edit.</div>
      <div className="gp-foot"><Button onClick={sheet.close}>Close</Button></div></>);
    return;
  }
  sheet.render(<ServerEditor title={title} harness={harness} server={server} close={sheet.close}
    save={save} pending={value => { busy = value; }} />);
}

function ServerEditor({ title, harness, server, save, pending, close }: {
  title: string; harness: Harness; server: Server | null;
  save: (server: EditableServer) => Promise<SaveStatus>; pending: (busy: boolean) => void; close: () => void;
}) {
  const settings = server?.settings ?? null;
  const stdio = settings && "stdio" in settings.connection ? settings.connection.stdio : null;
  const remote = settings && "remote" in settings.connection ? settings.connection.remote : null;
  const codex = harness === "codex";
  const [transport, setTransport] = useState<Transport>(remote?.transport ?? "stdio");
  const [args, setArgs] = useState(() => rows((stdio?.args ?? []).map(value => ["", value])));
  const [env, setEnv] = useState(() => rows(Object.entries(stdio?.env ?? {})));
  const [headers, setHeaders] = useState(() => rows(Object.entries(remote?.headers ?? {})));
  const [envHeaders, setEnvHeaders] = useState(() => rows(Object.entries(remote?.env_http_headers ?? {})));
  const [command, setCommand] = useState(stdio?.command ?? "");
  const [cwd, setCwd] = useState(stdio?.cwd ?? "");
  const [url, setUrl] = useState(remote?.url ?? "");
  const [token, setToken] = useState(remote?.bearer_token_env_var ?? "");
  const [enabled, setEnabled] = useState(settings?.enabled ?? true);
  const [options, setOptions] = useState<CodexOptions>(settings?.codex ?? {
    required: null, startup_timeout_sec: null, tool_timeout_sec: null, enabled_tools: null,
    disabled_tools: null, default_tools_approval_mode: null,
  });
  const [includeTools, setIncludeTools] = useState(options.enabled_tools !== null);
  const [allowed, setAllowed] = useState(() => listRows(options.enabled_tools ?? []));
  const [blocked, setBlocked] = useState(() => listRows(options.disabled_tools ?? []));
  const [oauth, setOauth] = useState<ClaudeOauth>(settings?.claude_oauth ?? {
    clientId: null, callbackPort: null, authServerMetadataUrl: null, scopes: null,
  });
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [error, setError] = useState("");
  const claudeHttp = harness === "claude" && (transport === "http" || transport === "sse");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || conflict) return;
    const name = String(new FormData(event.currentTarget).get("name") ?? "").trim();

    let connection: Settings["connection"];
    try {
      connection = transport === "stdio"
        ? { stdio: { command, args: args.map(row => row.value), env: pairs(env), cwd: codex ? cwd || null : null } }
        : { remote: { transport, url: url.trim(), headers: pairs(headers),
          env_http_headers: codex ? pairs(envHeaders) : {}, bearer_token_env_var: codex ? token.trim() || null : null } };
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : reason(failure));
      return;
    }

    pending(true);
    setBusy(true);
    setError("");

    try {
      const status = await save({ name, settings: {
        connection, enabled,
        codex: codex ? {
          ...options,
          enabled_tools: includeTools ? allowed.map(row => row.value.trim()) : null,
          disabled_tools: blocked.length ? blocked.map(row => row.value.trim()) : options.disabled_tools?.length === 0 ? [] : null,
        } : null,
        claude_oauth: claudeHttp && Object.values(oauth).some(value => value !== null) ? oauth : null,
      } });
      if (status === "saved") {
        close();
      } else {
        setConflict(true);
      }
    } catch (failure) {
      setError(reason(failure));
    } finally {
      pending(false);
      setBusy(false);
    }
  }

  const transports: Transport[] = harness === "claude" ? ["stdio", "http", "sse", "ws"] : ["stdio", "http"];
  return <>
    <SheetHead title={<>{title}<small>{HARNESS[harness]} user configuration</small></>} />
    <form className="dform mcp-editor" onSubmit={event => void submit(event)} autoComplete="off">
      <label className="dfield">Name<input name="name" defaultValue={server?.name ?? ""} required maxLength={128} disabled={busy} {...NO_AUTO} autoFocus /></label>
      <div className="dfield">Transport
        <Picker<Transport> label="Transport" field disabled={busy} options={transports.map(item => ({ id: item, label: TRANSPORT[item] }))} value={transport} onChange={setTransport} />
      </div>
      {transport === "stdio" ? <>
        <StringField label="Command" value={command} change={setCommand} required busy={busy} />
        {codex && <StringField label="Working directory" value={cwd} change={setCwd} busy={busy} />}
        <FieldRows label="Arguments" item="argument" valueLabel="Argument" rows={args} change={setArgs} busy={busy} />
        <FieldRows label="Environment variables" item="variable" nameLabel="Variable name" secret rows={env} change={setEnv} busy={busy} />
      </> : <>
        <StringField label="Server URL" value={url} change={setUrl} required busy={busy} />
        <FieldRows label="Headers" item="header" nameLabel="Header name" secret rows={headers} change={setHeaders} busy={busy} />
        {codex && <>
          <StringField label="Bearer token variable" value={token} change={setToken} busy={busy} />
          <FieldRows label="Headers from environment variables" item="header variable" nameLabel="Header name" valueLabel="Variable name" rows={envHeaders} change={setEnvHeaders} busy={busy} />
        </>}
      </>}
      {harness !== "claude" && <label className="dline"><Checkbox checked={enabled} change={setEnabled} disabled={busy} /> Enabled</label>}
      {codex && <details className="mcp-options">
        <summary>Startup and tool settings</summary>
        <div className="dform">
          <label className="dline"><Checkbox checked={options.required ?? false} disabled={busy}
            change={required => setOptions({ ...options, required })} /> Required at startup</label>
          <NumberField label="Startup timeout (seconds)" value={options.startup_timeout_sec} busy={busy}
            change={startup_timeout_sec => setOptions({ ...options, startup_timeout_sec })} />
          <NumberField label="Tool timeout (seconds)" value={options.tool_timeout_sec} busy={busy}
            change={tool_timeout_sec => setOptions({ ...options, tool_timeout_sec })} />
          <div className="dfield">Default tool approval
            <Picker<Approval | "default"> label="Default tool approval" field disabled={busy} value={options.default_tools_approval_mode ?? "default"}
              options={[{ id: "default", label: "Use Codex default" }, ...approvalModes.map(mode => ({ id: mode, label: APPROVAL[mode] }))]}
              onChange={value => setOptions({ ...options, default_tools_approval_mode: value === "default" ? null : value })} />
          </div>
          <label className="dline"><Checkbox checked={includeTools} change={setIncludeTools} disabled={busy} /> Only allow listed tools</label>
          {includeTools && <FieldRows label="Allowed tools" item="allowed tool" valueLabel="Tool name" rows={allowed} change={setAllowed} busy={busy} />}
          <FieldRows label="Blocked tools" item="blocked tool" valueLabel="Tool name" rows={blocked} change={setBlocked} busy={busy} />
        </div>
      </details>}
      {claudeHttp && <details className="mcp-options">
        <summary>OAuth settings</summary>
        <div className="dform">
          <StringField label="Client ID" value={oauth.clientId ?? ""} busy={busy} change={clientId => setOauth({ ...oauth, clientId: clientId || null })} />
          <NumberField label="Callback port" value={oauth.callbackPort} busy={busy} integer
            change={callbackPort => setOauth({ ...oauth, callbackPort })} />
          <StringField label="Authorization server metadata URL" value={oauth.authServerMetadataUrl ?? ""} busy={busy}
            change={authServerMetadataUrl => setOauth({ ...oauth, authServerMetadataUrl: authServerMetadataUrl || null })} />
          <StringField label="Scopes" value={oauth.scopes ?? ""} busy={busy} change={scopes => setOauth({ ...oauth, scopes: scopes || null })} />
        </div>
      </details>}
      {conflict && <div className="derr" role="alert">This server changed or was removed while you were editing. Close the editor to see the latest configuration.</div>}
      {error && <div className="derr" role="alert">{error}</div>}
      <div className="gp-foot">
        <Button disabled={busy} look="quiet" onClick={close}>Cancel</Button>
        <Button disabled={busy || conflict} type="submit" look="primary">{busy ? "Saving…" : "Save server"}</Button>
      </div>
    </form>
  </>;
}

const approvalModes: Approval[] = ["auto", "prompt", "writes", "approve"];

function serverDetail(settings: Settings | null): string {
  if (!settings) return "Unsupported configuration";
  const transport = "stdio" in settings.connection ? "stdio" : settings.connection.remote.transport;
  return TRANSPORT[transport] + (settings.enabled ? "" : " · Disabled");
}

function StringField({ label, value, change, busy, required = false }: {
  label: string; value: string; change(value: string): void; busy: boolean; required?: boolean;
}) {
  return <label className="dfield">{label}<input className="url" value={value} required={required}
    onChange={event => change(event.currentTarget.value)} disabled={busy} {...NO_AUTO} /></label>;
}

function NumberField({ label, value, change, busy, integer = false }: {
  label: string; value: number | null; change(value: number | null): void; busy: boolean; integer?: boolean;
}) {
  return <label className="dfield">{label}
    <input type="number" step={integer ? 1 : "any"} value={value ?? ""} disabled={busy} onChange={event => {
      const value = event.currentTarget.valueAsNumber;
      change(Number.isFinite(value) ? value : null);
    }} />
  </label>;
}

interface FieldRow { id: number; name: string; value: string }
function listRows(values: string[]): FieldRow[] {
  return rows(values.map(value => ["", value]));
}

function rows(entries: [string, string][]): FieldRow[] {
  return entries.map(([name, value], id) => ({ id, name, value }));
}

function pairs(entries: FieldRow[]) {
  const result = new Map<string, string>();
  for (const { name, value } of entries) {
    const key = name.trim();
    if (result.has(key)) throw new Error(`${key} is listed twice.`);
    result.set(key, value);
  }
  return Object.fromEntries(result);
}

function FieldRows({ label, item, rows, change, busy, nameLabel = null, valueLabel = "Value", secret = false }: {
  label: string; item: string; rows: FieldRow[]; change(rows: FieldRow[]): void; busy: boolean;
  nameLabel?: string | null; valueLabel?: string; secret?: boolean;
}) {
  const update = (id: number, field: "name" | "value", value: string) =>
    change(rows.map(row => row.id === id ? { ...row, [field]: value } : row));

  return <fieldset className="dfield mcp-fields">
    <legend>{label}</legend>
    {rows.map((row, index) => <div className="dline tight" key={row.id}>
      {nameLabel !== null && <input required aria-label={`${nameLabel} ${index + 1}`} placeholder={nameLabel} value={row.name}
        onChange={event => update(row.id, "name", event.currentTarget.value)} disabled={busy} {...NO_AUTO} />}
      <input type={secret ? "password" : "text"} aria-label={`${valueLabel} ${index + 1}`} placeholder={valueLabel} value={row.value}
        onChange={event => update(row.id, "value", event.currentTarget.value)} disabled={busy} {...NO_AUTO} />
      <Button small look="quiet" disabled={busy} aria-label={`Remove ${item} ${index + 1}`}
        onClick={() => change(rows.filter(entry => entry.id !== row.id))}>×</Button>
    </div>)}
    <div><Button small look="quiet" disabled={busy} onClick={() => change([...rows, { id: (rows.at(-1)?.id ?? -1) + 1, name: "", value: "" }])}>+ Add {item}</Button></div>
  </fieldset>;
}
