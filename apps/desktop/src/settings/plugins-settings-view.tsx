import { createElement, useEffect, useLayoutEffect, useRef } from "react";
import { confirmSheet } from "../ui/menu.tsx";
import { openSheet } from "../ui/parts.tsx";
import { useStore } from "zustand";
import { Icon } from "../ui/icons.tsx";
import { NO_AUTO, Spinner } from "../ui/kit.tsx";
import type { PluginsSettings, PluginsState } from "./plugins-settings.ts";
import {
  consentAction, consentTitle, consentVersions, isNewPermission, permissionLabel, permissionMeaning, rowOffers, runLine,
  sourceLabel, sourceSentence, updateLine, type Flow, type PluginFailure,
} from "./plugins-settings-state.ts";
import type { PluginPermission, PluginRow } from "../ui/plugin-rows.ts";

type Consenting = Extract<Flow, { kind: "consent" }>;

/// The Plugins page of Settings.
export function PluginsView({ store }: { store: PluginsSettings }) {
  const state = useStore(store.state);
  useEffect(() => {
    void store.load();
    const stopListening = store.follow();
    const stopSheets = showConsentSheets(store);
    return () => { stopListening(); stopSheets(); };
  }, [store]);

  const { list, flow } = state;
  const working = flow.kind === "fetching" || flow.kind === "working" || flow.kind === "consent";
  const empty = list.kind === "ready" && list.rows.length === 0;
  return <div className="plg-page">
    <p className="plg-intro">Plugins add to the interface of this computer. A plugin runs only after you allow what it asks for, and you can turn it off or remove it here.</p>
    {list.kind === "ready" && !empty && <Actions store={store} working={working} />}
    {empty && <Empty store={store} working={working} />}
    {(flow.kind === "git" || flow.kind === "fetching") && <GitForm store={store} flow={flow} />}
    {flow.kind === "working" && <div className="dmuted plg-working" role="status"><Spinner />Reading the plugin. Nothing is installed yet.</div>}
    {flow.kind === "idle" && flow.failure !== null && <Failure failure={flow.failure} />}
    <Rows store={store} state={state} working={working} />
  </div>;
}

function Actions({ store, working }: { store: PluginsSettings; working: boolean }) {
  return <div className="dline tight wrap plg-top">
    <button type="button" className="dbtn small" disabled={working} onClick={store.installFromGit}>Install from Git…</button>
    <button type="button" className="dbtn small" disabled={working} onClick={() => { void store.installFromFolder(); }}>Install from folder…</button>
    <span className="dpush"><button type="button" className="dbtn quiet small" onClick={() => { void store.openFolder(); }}>Open plugins folder</button></span>
  </div>;
}

function Empty({ store, working }: { store: PluginsSettings; working: boolean }) {
  return <div className="plg-empty"><b>No plugins yet</b>
    <p>Plugins are small scripts that add to Beep's interface, such as a cat on the message box. Install one from a Git repository or from a folder on this computer.</p>
    <div className="dline tight wrap">
      <button type="button" className="dbtn pri small" disabled={working} onClick={store.installFromGit}>Install from Git…</button>
      <button type="button" className="dbtn small" disabled={working} onClick={() => { void store.installFromFolder(); }}>Install from folder…</button>
      <button type="button" className="dbtn quiet small" onClick={() => { void store.openFolder(); }}>Open plugins folder</button>
    </div>
  </div>;
}

function Failure({ failure }: { failure: PluginFailure }) {
  return <div className="derr" role="alert">{failure.message}</div>;
}

function GitForm({ store, flow }: { store: PluginsSettings; flow: Extract<Flow, { kind: "git" | "fetching" }> }) {
  const fetching = flow.kind === "fetching";
  const failure = flow.kind === "git" ? flow.failure : null;
  const first = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { first.current?.focus(); }, []);
  return <form className="plg-form" aria-label="Install from Git" onSubmit={event => { event.preventDefault(); void store.submitGit(); }}>
    <div className="dline"><b>Install from Git</b></div>
    <label className={`dfield${failure ? " err" : ""}`}><span>Repository</span>
      <input ref={first} className="url" value={flow.repository} readOnly={fetching} placeholder="owner/repo or https://github.com/owner/repo.git" {...NO_AUTO} onChange={event => store.editGit("repository", event.currentTarget.value)} />
    </label>
    <div className="plg-two">
      <label className="dfield"><span>Branch or tag (optional)</span><input value={flow.reference} readOnly={fetching} placeholder="main" {...NO_AUTO} onChange={event => store.editGit("reference", event.currentTarget.value)} /></label>
      <label className="dfield"><span>Folder in the repository (optional)</span><input value={flow.folder} readOnly={fetching} placeholder="plugins/cat" {...NO_AUTO} onChange={event => store.editGit("folder", event.currentTarget.value)} /></label>
    </div>
    {failure && <Failure failure={failure} />}
    {fetching && <div className="dmuted" role="status"><Spinner />Fetching the repository. Nothing is installed yet.</div>}
    <div className="dline tight">
      <button type="submit" className="dbtn pri small" disabled={fetching}>{fetching ? "Fetching…" : "Review plugin"}</button>
      <button type="button" className="dbtn quiet small" disabled={fetching} onClick={store.close}>Cancel</button>
    </div>
  </form>;
}

function Rows({ store, state, working }: { store: PluginsSettings; state: PluginsState; working: boolean }) {
  const { list } = state;
  if (list.kind === "loading") return <div className="dskel-rows" aria-busy="true"><div className="dskel" style={{ width: "78%" }} /><div className="dskel" style={{ width: "56%" }} /><div className="dskel" style={{ width: "68%" }} /></div>;
  if (list.kind === "failed") return <div className="plg-failed"><Failure failure={list.failure} /><div className="dline"><button type="button" className="dbtn small" onClick={() => { void store.load(); }}>Try again</button></div></div>;
  return <div className="plg-list" role="list" aria-label="Plugins">{list.rows.map(row => <Row key={row.id} row={row} store={store} state={state} working={working} />)}</div>;
}

function Row({ row, store, state, working }: { row: PluginRow; store: PluginsSettings; state: PluginsState; working: boolean }) {
  const offers = rowOffers(row), line = runLine(row.run, row.style.size > 0);
  const busy = state.busy.includes(row.id);
  const failure = state.failures.get(row.id) ?? null, check = state.updates.get(row.id) ?? null;
  return <div className={`dgit plg${offers.on ? "" : " off"}`} role="listitem" data-plugin={row.id}>
    <span className="ic"><Icon name="plugin" /></span>
    <div className="l1"><b>{row.name}</b><span className="acct">{row.version}</span>
      <button type="button" className="dswitch" role="switch" aria-label={row.name} aria-checked={offers.on} disabled={busy || offers.switchLocked} title={offers.switchLocked ? "Allow this plugin first" : undefined} onClick={() => { void store.setEnabled(row.id, !offers.on); }} />
    </div>
    {row.description !== "" && <span className="plg-desc">{row.description}</span>}
    <div className="l2">{row.permissions.length === 0 ? <span className="dgchip">Asks for no permissions</span> : row.permissions.map(permission => <PermissionChip key={permission} permission={permission} />)}</div>
    <div className="dline plg-foot">
      <span className={`plg-state ${line.tone}`}>{line.text}</span><span className="dsub plg-src">{sourceLabel(row.source)}</span>
      <span className="acts">
        {offers.allow && <button type="button" className="dbtn pri small" aria-label={`Allow ${row.name}`} disabled={working || busy} onClick={() => { void store.allow(row.id); }}>Allow…</button>}
        {offers.update && <button type="button" className="dbtn small" aria-label={`Update ${row.name}`} disabled={working || busy || check?.kind === "checking"} onClick={() => { void store.update(row.id); }}>Update</button>}
        {offers.remove && <button type="button" className="dbtn quiet small" aria-label={`Remove ${row.name}`} disabled={busy} onClick={() => askRemove(store, row)}>Remove…</button>}
      </span>
    </div>
    {check !== null && <span className={`dsub plg-note${check.kind === "failed" ? " bad" : ""}`} role={check.kind === "failed" ? "alert" : "status"}>{updateLine(check)}</span>}
    {failure !== null && <Failure failure={failure} />}
  </div>;
}

function PermissionChip({ permission }: { permission: PluginPermission }) {
  return <span className="dgchip" title={permissionMeaning(permission)}>{permissionLabel(permission)}</span>;
}

/// What the consent sheet shows: what the plugin is, what it asks for, and what changed.
export function ConsentView({ flow, confirm, cancel }: { flow: Consenting; confirm(): void; cancel(): void }) {
  const { preview } = flow.consent;
  const action = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { action.current?.focus(); }, []);
  return <div className="plg-sheet">
    <div className="am-head"><b>{consentTitle(preview)}</b><kbd>esc</kbd></div>
    <div className="plg-meta"><b>{consentVersions(preview)}</b><br />{sourceSentence(preview.source)}</div>
    {preview.description !== "" && <div className="plg-meta">{preview.description}</div>}
    {preview.permissions.length === 0
      ? <div className="plg-perms none">This plugin asks for no permissions.</div>
      : <><div className="am-label">It asks to:</div><ul className="plg-perms">{preview.permissions.map(permission => <li key={permission}>
        <b>{permissionLabel(permission)}{isNewPermission(preview, permission) && <span className="dgchip warn">New</span>}</b><small>{permissionMeaning(permission)}</small>
      </li>)}</ul></>}
    {preview.kind === "update" && preview.removed.length > 0 && <div className="plg-removed">No longer asked for: {preview.removed.map(permissionLabel).join(", ")}.</div>}
    <div className="am-sub">A plugin is a script that runs on this computer in a sandbox. It can only do what is listed here.</div>
    {flow.failure !== null && <div className="am-msg bad" role="alert">{flow.failure.message}</div>}
    <div className="gp-foot">
      <button type="button" className="dbtn quiet" disabled={flow.busy} onClick={cancel}>Cancel</button>
      <button ref={action} type="button" className="dbtn pri" disabled={flow.busy} onClick={confirm}>{flow.busy ? "Working…" : consentAction(preview)}</button>
    </div>
  </div>;
}

/// Asks before removing a plugin, naming it.
function askRemove(store: PluginsSettings, row: PluginRow) {
  confirmSheet({
    title: `Remove ${row.name}?`,
    text: "Beep stops the plugin and deletes its folder from the plugins folder.",
    note: "You can install it again later.",
    action: "Remove",
    run: () => store.remove(row.id),
  });
}

/// Keeps the consent sheet open while the flow holds a consent, and draws it from the store.
function showConsentSheets(store: PluginsSettings) {
  let sheet: ReturnType<typeof openSheet> | null = null;
  const draw = () => {
    const { flow } = store.state.getState();
    if (flow.kind !== "consent") {
      const open = sheet;
      sheet = null;
      open?.close();
      return;
    }

    sheet ??= openSheet("Plugin permissions", "am-sheet", {
      canClose: () => { const now = store.state.getState().flow; return now.kind !== "consent" || !now.busy; },
      onClose: () => { if (sheet !== null) { sheet = null; store.close(); } },
    });
    sheet.render(createElement(ConsentView, { flow, confirm: () => { void store.confirm(); }, cancel: store.close }), consentTitle(flow.consent.preview));
  };

  draw();
  const stop = store.state.subscribe(draw);

  return () => {
    stop();
    if (sheet !== null) store.close();
    const open = sheet;
    sheet = null;
    open?.close();
  };
}
