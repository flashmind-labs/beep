import { useEffect } from "react";
import { Button as AriaButton } from "react-aria-components/Button";
import { Header } from "react-aria-components/Header";
import { Menu, MenuItem, MenuSection, MenuTrigger } from "react-aria-components/Menu";
import { Popover } from "react-aria-components/Popover";
import { Separator } from "react-aria-components/Separator";
import { useStore } from "zustand";
import { skillSync, projectSync, dockerContainers, type SyncSettingKind } from "../../stores/settings/skill-sync.ts";
import type { MacSettings } from "./settings.tsx";
import { Button, Switch } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";
import { bytes, spoken } from "../../ui/format.ts";
import { useDialogRef } from "../../ui/parts.tsx";
import { SettingRow } from "./overview-view.tsx";
import { useBuildCapabilities } from "../../ui/tauri.ts";
import { dictationSettings } from "../../parts/session/dictation.tsx";
import {
  activeModel,
  DICTATION_MODELS,
  dictationNote,
  localModelStatus,
  PROVIDER_NAMES,
  providerOf,
  type DictationKeys,
  type LocalModels,
} from "../../stores/chat/dictation-state.ts";

interface Props {
  settings: MacSettings; phone: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; notices(enabled: boolean): void; optionTaps(enabled: boolean): void; forgeNotices(enabled: boolean): void; nodeUpdates(enabled: boolean): void; developer(enabled: boolean): void; chooseSaves(): void; useDownloads(): void; shortcuts(): void; report(): void; credentials(): void;
}
const PREFERENCES: Record<SyncSettingKind, { store: typeof skillSync; id: string; title: string; loading: string; on: string; off: string }> = {
  skills: { store: skillSync, id: "skillSyncSetting", title: "Sync coding agent skills", loading: "Loading skill sync setting...", on: "Keep your coding agent skills in sync across your machines.", off: "Skill syncing is paused on all your machines. Existing files stay in place." },
  projects: { store: projectSync, id: "projectSyncSetting", title: "Project sync", loading: "Loading project sync setting...", on: "Sync projects across all your machines, in place of Syncthing. Turning off keeps existing files in place.", off: "Paused for every project on all machines. Turn on to reconcile your projects." },
  docker: { store: dockerContainers, id: "dockerContainersSetting", title: "Docker containers in Running", loading: "Loading Docker setting...", on: "Sessions list the containers their agent starts, with their ports. Applies to new sessions.", off: "Sessions list only their own processes. Applies to new sessions." },
};
const LOCAL_MODELS = DICTATION_MODELS.filter(model => model.provider === "local");
const CLOUD_MODELS = DICTATION_MODELS.filter(model => model.provider !== "local");

function downloadPct(downloadedBytes: number, totalBytes: number): number {
  return totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : 0;
}

function SyncPreference({ kind }: { kind: SyncSettingKind }) {
  const { store, id, title, loading, on, off } = PREFERENCES[kind];
  const state = useStore(store.state);
  useEffect(() => { void store.load(); }, [store]);
  const note = state.enabled === null ? loading : state.enabled ? on : off;
  return <SettingRow id={id} title={title} note={note} extra={state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <Button look="quiet" onClick={() => void store.load()}>Retry</Button></>}</span>}><Switch label={title} on={state.enabled ?? false} disabled={state.busy || state.enabled === null} flip={() => void store.change()} /></SettingRow>;
}

function DictationModelPicker({ model, keys, localModels, disabled, onChange }: {
  model: string | null;
  keys: DictationKeys;
  localModels: LocalModels;
  disabled: boolean;
  onChange(next: string | null): void;
}) {
  const dialog = useDialogRef<HTMLDivElement>();
  const ready = activeModel(null, keys, localModels);
  const picked = model === null ? null : DICTATION_MODELS.find(item => item.id === model) ?? null;
  const pickedProvider = model === null ? null : providerOf(model);
  const badge = model === null ? "Auto" : pickedProvider ? PROVIDER_NAMES[pickedProvider] : "Model";
  const label = model === null ? (ready?.name ?? "Automatic") : (picked?.name ?? model);
  const currentKey = model ?? "auto";

  return <MenuTrigger>
    <AriaButton id="dictationModel" className="dict-trigger" isDisabled={disabled} aria-label={spoken("Dictation model", badge, label)}>
      <span className={pickedProvider === "local" ? "src local" : "src"}>{badge}</span>
      <span>{label}</span>
      <Icon name="caretDown" size={12} />
    </AriaButton>
    <Popover ref={dialog} className="dict-popover" placement="bottom start" offset={6}>
      <Menu className="dict-menu" aria-label="Dictation model" selectionMode="single" disallowEmptySelection selectedKeys={[currentKey]}
        onAction={key => onChange(key === "auto" ? null : String(key))}>
        <MenuItem id="auto" className="dict-opt" textValue="Automatic First ready live model">
          <span className="chk" aria-hidden="true"><Icon name="check" size={12} /></span>
          <span className="main"><span className="nm">Automatic</span><span className="meta">First ready live model</span></span>
          <span className="tag dim">Auto</span>
        </MenuItem>
        <Separator className="dict-sep" />
        <MenuSection className="dict-sec">
          <Header className="dict-sec-hd">On this computer</Header>
          {LOCAL_MODELS.map(item => {
            const status = localModelStatus(item.id, localModels);
            const total = status?.bytes ?? item.bytes ?? 0;
            const pct = status ? downloadPct(status.downloadedBytes, total) : 0;
            return <MenuItem id={item.id} key={item.id} className="dict-opt" textValue={`${item.name} ${item.meta}`}>
              <span className="chk" aria-hidden="true"><Icon name="check" size={12} /></span>
              <span className="main"><span className="nm">{item.name}</span><span className="meta">{item.meta}</span></span>
              {status?.installed
                ? <span className="tag ready">On device</span>
                : status?.downloading
                  ? <span className="tag">{pct}%</span>
                  : <span className="tag">{bytes(total)}</span>}
            </MenuItem>;
          })}
        </MenuSection>
        <Separator className="dict-sep" />
        <MenuSection className="dict-sec">
          <Header className="dict-sec-hd">Cloud live API</Header>
          {CLOUD_MODELS.map(item => {
            const key = item.provider === "local" ? undefined : keys[item.provider];
            return <MenuItem id={item.id} key={item.id} className="dict-opt" textValue={`${item.name} ${item.meta}`}>
              <span className="chk" aria-hidden="true"><Icon name="check" size={12} /></span>
              <span className="main"><span className="nm">{item.name}</span><span className="meta">{item.meta}</span></span>
              {key !== undefined
                ? <span className="tag">{key ? `…${key}` : "Saved"}</span>
                : <span className="tag dim">No key</span>}
            </MenuItem>;
          })}
        </MenuSection>
      </Menu>
    </Popover>
  </MenuTrigger>;
}

/// Voice dictation for every desktop: the switch, the live model menu, and local download or cloud API key status.
function DictationPreference({ credentials }: { credentials(): void }) {
  const state = useStore(dictationSettings.state);
  useEffect(() => { void dictationSettings.load(); }, []);
  const saved = state.saved;
  const title = "Voice dictation";
  if (saved === null) return <SettingRow id="dictationSetting" title={title} note={state.error ? "" : "Loading dictation setting..."} extra={state.error && <span className="derr" role="alert">{state.error} <Button look="quiet" onClick={() => void dictationSettings.load()}>Retry</Button></span>}><Switch label={title} on={false} disabled flip={() => {}} /></SettingRow>;

  const pickedProvider = saved.model === null ? null : providerOf(saved.model);
  const ready = activeModel(saved.model, state.keys, state.localModels);
  const renderActions = () => {
    if (saved.model !== null && pickedProvider === "local") {
      const modelId = saved.model;
      const status = localModelStatus(modelId, state.localModels);
      const total = status?.bytes ?? 0;
      if (status?.downloading) {
        const pct = downloadPct(status.downloadedBytes, total);
        return <>
          <span className="fs-pct" aria-label={`Download progress ${pct}%`}><span className="fs-bar"><i style={{ width: `${pct}%` }} /></span>{pct}%</span>
          <Button look="quiet" small onClick={() => void dictationSettings.cancelDownload(modelId)}>Cancel</Button>
        </>;
      }
      if (status?.installed) return <span className="dgchip ready">On device · {bytes(total)}</span>;
      return <Button look="primary" small onClick={() => void dictationSettings.downloadModel(modelId)}>Download ({bytes(total)})</Button>;
    }

    if (saved.model === null && ready?.provider === "local") {
      const status = localModelStatus(ready.id, state.localModels);
      return <span className="dgchip ready">On device · {bytes(status?.bytes ?? ready.bytes ?? 0)}</span>;
    }

    const cloud = pickedProvider && pickedProvider !== "local"
      ? pickedProvider
      : ready && ready.provider !== "local"
        ? ready.provider
        : null;
    const key = cloud ? state.keys[cloud] : undefined;
    return <>
      {cloud && key !== undefined ? <span className="dgchip">{PROVIDER_NAMES[cloud]} key{key ? ` …${key}` : ""}</span> : null}
      <Button look={cloud && key !== undefined ? "quiet" : "secondary"} small onClick={credentials}>{cloud && key !== undefined ? "Credentials…" : "Add API key…"}</Button>
    </>;
  };

  const controls = saved.enabled && <span className="dict-sub">
    <DictationModelPicker model={saved.model} keys={state.keys} localModels={state.localModels} disabled={state.busy} onChange={next => dictationSettings.setModel(next)} />
    {renderActions()}
  </span>;
  return <SettingRow id="dictationSetting" title={title} note={dictationNote(saved, state.keys, state.localModels)} extra={<>{controls}{state.error && <span className="derr" role="alert">{state.error}</span>}</>}><Switch label={title} on={saved.enabled} disabled={state.busy} flip={() => dictationSettings.setEnabled(!saved.enabled)} /></SettingRow>;
}

export function GeneralView(p: Props) {
  const s = p.settings;
  const capabilities = useBuildCapabilities();
  return <>
    {!p.phone && capabilities.autostart && <SettingRow title="Launch at login" note="Keep Beep in the tray after a restart"><Switch id="autostart" label="Launch at login" on={s.autostart} disabled={p.busy.size > 0} flip={() => p.autostart(!s.autostart)} /></SettingRow>}
    {!p.phone && capabilities.option_taps && <SettingRow title="Open with Option twice" note={s.option_taps ? "Tap Option twice in any app to open or hide the quick agent" : "Open the quick agent from the tray menu"}><Switch id="optionTaps" label="Open with Option twice" on={s.option_taps} disabled={p.busy.size > 0} flip={() => p.optionTaps(!s.option_taps)} /></SettingRow>}
    <SettingRow title="Coding agent notifications" note={s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}><Switch id="agentNotices" label="Coding agent notifications" on={s.agent_notices} disabled={p.busy.size > 0} flip={() => p.notices(!s.agent_notices)} /></SettingRow>
    <SettingRow title="Pull request notifications" note={s.forge_notices ? "When someone asks for your review or assigns you a pull request or issue" : "Review requests and assignments only show on Home"}><Switch id="forgeNotices" label="Pull request notifications" on={s.forge_notices} disabled={p.busy.size > 0} flip={() => p.forgeNotices(!s.forge_notices)} /></SettingRow>
    <SyncPreference kind="docker" />
    {/* A phone hosts no files and has no keyboard: these belong to the desktops. */}
    {!p.phone && <><DictationPreference credentials={p.credentials} /><SyncPreference kind="skills" /><SyncPreference kind="projects" /><SettingRow title="Save files to" note={s.save_folder ?? "No folder"}><span>{s.save_folder_picked && <Button look="quiet" id="useDownloads" disabled={p.busy.size > 0} onClick={p.useDownloads}>Use Downloads</Button>}{capabilities.local_host && <Button id="chooseSaves" disabled={p.busy.size > 0} onClick={p.chooseSaves}>Choose…</Button>}</span></SettingRow><SettingRow title="Keyboard shortcuts" note="Every shortcut, by where it works"><Button id="openShortcuts" onClick={p.shortcuts}>Show…</Button></SettingRow>
      <SettingRow title="Update machines automatically" note={s.node_auto_update ? "Each machine updates beep-node when it is seen online, after its agents finish their turns" : "A notification says when a machine is behind"}><Switch id="nodeAutoUpdate" label="Update machines automatically" on={s.node_auto_update} disabled={p.busy.size > 0} flip={() => p.nodeUpdates(!s.node_auto_update)} /></SettingRow>
      <SettingRow title="Developer mode" note={s.developer ? "Updates to every release, patches included" : "Updates to minor releases only"}><Switch id="developerMode" label="Developer mode" on={s.developer} disabled={p.busy.size > 0} flip={() => p.developer(!s.developer)} /></SettingRow></>}<SettingRow title="Report a problem" note="Collect logs from your machines and send them to us"><Button id="openReport" onClick={p.report}>Report…</Button></SettingRow>
  </>;
}
