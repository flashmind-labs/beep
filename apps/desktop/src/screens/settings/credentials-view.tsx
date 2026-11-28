import { Button, GitChip, NO_AUTO, Picker, SkeletonRows, Spinner } from "../../ui/kit.tsx";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { apiKeys, forgejoTokenPage, ghChoices, unsavedProviders, githubAccounts, hostOf, teaHas, teaOffers, GITHUB_TOKEN_PAGE, type ApiKey, type CredCheck, type Credential, type CredState, type GitHubAccount } from "../../stores/settings/credentials.ts";
import { Icon, type IconName } from "../../ui/icons.tsx";

type Machines = { name: string; env?: { tools?: { name: string }[] } | null }[] | null;
export interface CredentialActions {
  edit(key: string): void; cancel(): void; save(url: string, secret: string, clear: () => void): void; remove(id: string): Promise<void>;
  useCli(key: string): void; signIn(): void; cancelSignIn(): void; addAccount(open: boolean): void; useAccount(login: string): void; defaultAccount(id: string): Promise<void>; open(url: string): void;
}
function IconButton({ icon, label, onClick }: { icon: IconName; label: string; onClick: () => void | Promise<void> }) {
  const [busy, setBusy] = useState(false);
  return <Button look="quiet" className="dico" title={label} aria-label={label} disabled={busy} onClick={() => { const result = onClick(); if (result instanceof Promise) { setBusy(true); void result.finally(() => setBusy(false)); } }}><span><Icon name={icon} /></span></Button>;
}
function DefaultButton({ action }: { action: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  return <Button small disabled={busy} onClick={() => { setBusy(true); void action().finally(() => setBusy(false)); }}>Make default</Button>;
}
function CliChip({ cli, machines }: { cli: string; machines: Machines }) {
  if (!machines) return null;
  const names = machines.filter(m => m.env?.tools?.some(t => t.name === cli)).map(m => m.name);
  return <GitChip title={names.length ? names.join(", ") : undefined}>{names.length === 0 ? `${cli} on no machine yet` : names.length === 1 ? `${cli} on ${names[0]}` : `${cli} on ${names.length} machines`}</GitChip>;
}
function Link({ href, actions, children }: { href: string; actions: CredentialActions; children: ReactNode }) { return <a href={href} className="dlink" onClick={e => { e.preventDefault(); e.stopPropagation(); actions.open(href); }}>{children}</a>; }
function Form({ state, actions, title, subtitle, children }: { state: CredState; actions: CredentialActions; title: string; subtitle?: string; children: ReactNode }) {
  const secret = useRef<HTMLInputElement>(null), url = useRef<HTMLInputElement>(null), edit = state.edit, key = edit?.kind === "api_key";
  const providers = unsavedProviders(state), picking = key && edit.provider === null;
  const [provider, setProvider] = useState(() => providers[0]?.provider ?? null);
  useLayoutEffect(() => { (url.current ?? secret.current)?.focus(); }, []);
  const save = () => actions.save(url.current?.value ?? (picking ? provider : null) ?? "", secret.current?.value ?? "", () => { if (secret.current) secret.current.value = ""; });
  return <div className="dcred-form"><div className="dline"><b>{title}</b>{subtitle && <span className="dfine">{subtitle}</span>}</div><div className={`dfield${state.error ? " err" : ""}`}>
    {edit?.kind === "forgejo" && !edit.id && <input ref={url} id="credUrl" className="url" type="url" placeholder="https://git.example.com" {...NO_AUTO} defaultValue={edit.url ?? ""} aria-label="Server URL" />}
    <div className="dline">{picking && <Picker label="Provider" field className="dprovider" options={providers.map(p => ({ id: p.provider, label: p.name }))} value={provider} onChange={setProvider} />}<input ref={secret} id="credSecret" type="password" className="dsecret" placeholder={key ? "Paste the key" : "Paste the token"} {...NO_AUTO} aria-label={key ? "Key" : "Token"} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); save(); } }} /><Button look="primary" id="credSave" disabled={!!state.busy} onClick={save}>{key ? state.busy ? "Saving…" : "Save" : state.busy ? "Checking…" : "Check and save"}</Button><Button look="quiet" id="credCancel" onClick={actions.cancel}>Cancel</Button></div></div><span className="dsub">{children}</span><div className="derr" id="credErr">{state.error || ""}</div></div>;
}
function Waiting({ state, actions }: { state: CredState; actions: CredentialActions }) { const flow = state.ghFlow; if (!flow) return null; return <div className="dflow">{flow.code && <span>Enter code <code className="dflowcode">{flow.code}</code> on <Link href={flow.url} actions={actions}>GitHub</Link>{flow.copied && <span className="dmut"> (copied)</span>}</span>}<span className="dmuted"><Spinner />{flow.code ? "Waiting for GitHub" : "Starting gh…"}</span></div>; }
function Use({ label, busy, action }: { label: string; busy: boolean; action: () => void }) { return <Button look="primary" small disabled={busy} onClick={action}>{busy ? "Checking…" : label}</Button>; }
function GitRow({ icon, name, account, tag, usable, acts, chips, children }: { icon: IconName; name: string; account?: string; tag?: ReactNode; usable: boolean; acts: ReactNode; chips?: ReactNode; children?: ReactNode }) { return <div className={`dgit${usable ? "" : " off"}`}><span className="ic"><span><Icon name={icon} /></span></span><div className="l1"><b>{name}</b>{account && <span className="acct">{account}</span>}{tag}<span className="acts">{acts}</span></div>{chips && <div className="l2">{chips}</div>}{children}</div>; }
function CheckChip({ check, name, known }: { check: CredCheck | undefined; name: string; known: boolean }) {
  if (check?.state === "checking") return <GitChip>checking from your machines…</GitChip>;
  if (check?.state === "valid") return check.machine ? <GitChip title={`This device cannot reach ${name}; ${check.machine} can`}>checked from {check.machine}</GitChip> : null;
  if (check?.state === "refused") return <GitChip kind="err" title={check.message}>token refused</GitChip>;
  if (check?.state === "unreachable") return <GitChip kind="warn" title={check.message}>not reachable from any of your machines</GitChip>;
  return known ? null : <GitChip kind="warn">not checked: Beep could not reach it</GitChip>;
}
function EditRemove({ actions, name, keyName, id }: { actions: CredentialActions; name: string; keyName: string; id: string }) { return <><IconButton icon="edit" label={`Replace the token for ${name}`} onClick={() => actions.edit(keyName)} /><IconButton icon="remove" label={`Remove ${name}`} onClick={() => actions.remove(id)} /></>; }
function Account({ account, state, machines, actions }: { account: GitHubAccount; state: CredState; machines: Machines; actions: CredentialActions }) {
  const usable = !account.unreadable, cli = state.cli, ghHere = cli?.github_accounts?.some(g => g.login.toLowerCase() === account.login.toLowerCase()) || cli?.github?.state === "signed_in", what = account.login ? `@${account.login}` : "GitHub", orgs = account.orgs ?? [], more = orgs.slice(3);
  return <GitRow icon="github" name="GitHub" account={account.login ? `@${account.login}` : ""} usable={usable} tag={account.default && <GitChip title="GH_TOKEN on your machines">default</GitChip>} acts={<>{!usable && ghHere && <Use label="Use gh sign-in" busy={state.cliBusy === "github"} action={() => actions.useCli("github")} />}{!account.default && <DefaultButton action={() => actions.defaultAccount(account.id)} />}<EditRemove actions={actions} name={what} keyName={`github:${account.id}`} id={account.id} /></>} chips={<>{!usable && <GitChip kind="err">Re-enter this token</GitChip>}{orgs.slice(0, 3).map(org => <GitChip key={org}>{org}</GitChip>)}{more.length > 0 && <GitChip title={more.join(", ")}>+{more.length}</GitChip>}<CliChip cli="gh" machines={machines} /></>}>
    {!usable && state.cliError?.key === "github" && <span className="derr">{state.cliError.message}</span>}
  </GitRow>;
}
function Choices({ state, accounts, actions }: { state: CredState; accounts: GitHubAccount[]; actions: CredentialActions }) { return <>{ghChoices(state.cli, accounts).map(g => <div className="dline dsm" key={g.login}><span className="dmut">gh on this computer as @{g.login}</span>{g.active && <span className="dsub">active</span>}<span className="dpush"><Use label="Use" busy={state.cliBusy === `gh:${g.login}`} action={() => actions.useAccount(g.login)} /></span></div>)}</>; }
function GithubForm({ state, actions, id }: { state: CredState; actions: CredentialActions; id: string }) { return <Form state={state} actions={actions} title="GitHub" subtitle={id === "new" ? "Another account" : "Replace the token"}>Make one on <Link href={GITHUB_TOKEN_PAGE} actions={actions}>GitHub</Link> with repo access, or run <code>gh auth login</code> in Terminal and use that sign-in instead.</Form>; }
function Github({ state, machines, actions, part }: { state: CredState; machines: Machines; actions: CredentialActions; part: "rows" | "adding" }) {
  const accounts = githubAccounts(state), edit = state.edit, gh = state.cli?.github, choices = ghChoices(state.cli, accounts);
  if (!accounts.length && part === "adding") return null;
  if (!accounts.length && edit?.kind === "github") return <GithubForm state={state} actions={actions} id={edit.id ?? "new"} />;
  if (!accounts.length) {
    const several = choices.length > 1 && !state.ghFlow;
    const account = several ? `gh on this computer has ${choices.length} accounts` : gh?.state === "signed_in" ? gh.account ? `gh on this computer as @${gh.account}` : "gh signed in on this computer" : "";
    const paste = <Button look={gh || several ? "quiet" : "secondary"} small onClick={() => actions.edit("github")}>Paste a token</Button>;
    return <GitRow icon="github" name="GitHub" account={account} usable={false} acts={state.ghFlow ? <Button look="quiet" small onClick={e => { e.currentTarget.disabled = true; actions.cancelSignIn(); }}>Cancel</Button> : <>{!several && gh?.state === "signed_in" && <Use label="Use gh sign-in" busy={state.cliBusy === "github"} action={() => actions.useCli("github")} />}{!several && gh?.state === "signed_out" && <Button look="primary" small onClick={actions.signIn}>Sign in with GitHub</Button>}{paste}</>} chips={<CliChip cli="gh" machines={machines} />}>
      {!several && !state.ghFlow && gh?.state === "signed_out" && <span className="dsub">Opens GitHub in your browser, then saves the sign-in here</span>}{!several && !state.ghFlow && gh?.state === "missing" && <span className="dsub">Make a token on <Link href={GITHUB_TOKEN_PAGE} actions={actions}>GitHub</Link>, then paste it.<br />Or install GitHub CLI (<code>brew install gh</code>) and sign in here</span>}{several && <div className="dgrid6"><Choices state={state} accounts={accounts} actions={actions} /></div>}<Waiting state={state} actions={actions} />{state.cliError?.key === "github" && <span className="derr">{state.cliError.message}</span>}
    </GitRow>;
  }
  const adding = edit?.kind === "github" && (!edit.id || edit.id === "new");
  if (part === "rows") return <>{accounts.map(account => edit?.kind === "github" && edit.id === account.id ? <GithubForm key={account.id} state={state} actions={actions} id={account.id} /> : <Account key={account.id} account={account} state={state} machines={machines} actions={actions} />)}</>;
  return <>{adding ? <GithubForm state={state} actions={actions} id="new" /> : (state.ghAdd || state.ghFlow) && <div className="dcred-form"><div className="dline"><b>Add another GitHub account</b><span className="dpush"><Button look="quiet" small onClick={e => { if (state.ghFlow) { e.currentTarget.disabled = true; actions.cancelSignIn(); } else actions.addAccount(false); }}>Cancel</Button></span></div><div className="dgrid6 dind">{state.ghFlow ? <Waiting state={state} actions={actions} /> : <><Choices state={state} accounts={accounts} actions={actions} /><div className="dline tight">{(gh?.state === "signed_in" || gh?.state === "signed_out") && <Button small onClick={actions.signIn}>Sign in with GitHub</Button>}<Button look={gh?.state === "signed_in" || gh?.state === "signed_out" ? "quiet" : "secondary"} small onClick={() => actions.edit("github:new")}>Paste a token</Button></div>{gh?.state === "missing" ? <span className="dsub">Make a token on <Link href={GITHUB_TOKEN_PAGE} actions={actions}>GitHub</Link> while signed in as the other account, then paste it</span> : (gh?.state === "signed_in" || gh?.state === "signed_out") && <span className="dsub">Sign in adds the account to gh here too.{(() => { const saved = accounts.filter(a => state.cli?.github_accounts?.some(g => g.login.toLowerCase() === a.login.toLowerCase())).map(a => `@${a.login}`); return saved.length ? ` ${saved.join(", ")} ${saved.length > 1 ? "are" : "is"} in Beep already.` : ""; })()}</span>}</>}{state.cliError?.key === "github" && <span className="derr">{state.cliError.message}</span>}</div></div>}
  </>;
}
function Forgejo({ credential, state, machines, actions }: { credential: Extract<Credential, { kind: "forgejo" }>; state: CredState; machines: Machines; actions: CredentialActions }) {
  const name = hostOf(credential.url), key = `forgejo:${credential.url}`, usable = !credential.unreadable, check = state.checks?.[credential.id];
  if (state.edit?.kind === "forgejo" && state.edit.id === credential.id) { const page = forgejoTokenPage(credential.url); return <Form state={state} actions={actions} title={name} subtitle="Replace the token">{page && <>Make one at <Link href={page} actions={actions}>{hostOf(page)}</Link>, with repository and issue access.</>}</Form>; }
  const login = credential.account || (check?.state === "valid" ? check.account : ""), error = state.cliError?.key === key ? state.cliError.message : check?.state === "refused" ? check.message : "";
  return <GitRow icon="forgejo" name={name} account={usable && login ? `@${login}` : ""} usable={usable} acts={<>{!usable && teaHas(state.cli, credential.url) && <Use label="Use teacli sign-in" busy={state.cliBusy === key} action={() => actions.useCli(key)} />}<EditRemove actions={actions} name={name} keyName={`forgejo:${credential.id}`} id={credential.id} /></>} chips={<>{!usable && <GitChip kind="err">Re-enter this token</GitChip>}{usable && <CheckChip check={check} name={name} known={!!credential.account} />}<CliChip cli="teacli" machines={machines} /></>}>{error && <span className="derr">{error}</span>}</GitRow>;
}
function ApiKeyRow({ credential, state, actions }: { credential: ApiKey; state: CredState; actions: CredentialActions }) {
  const provider = state.providers?.find(p => p.provider === credential.provider), name = provider?.name ?? credential.provider;
  if (state.edit?.kind === "api_key" && state.edit.id === credential.id) return <Form state={state} actions={actions} title={name} subtitle="Replace the key">{null}</Form>;
  return <GitRow icon="key" name={name} account={`…${credential.last4}`} usable={!credential.unreadable} acts={<><IconButton icon="edit" label={`Replace the key for ${name}`} onClick={() => actions.edit(`api_key:${credential.id}`)} /><IconButton icon="remove" label={`Remove ${name}`} onClick={() => actions.remove(credential.id)} /></>} chips={<>{credential.unreadable && <GitChip kind="err">Re-enter this key</GitChip>}<GitChip title="The env var your machines get">{credential.env_var}</GitChip></>} />;
}
function ApiKeys({ state, actions }: { state: CredState; actions: CredentialActions }) {
  const keys = apiKeys(state), adding = state.edit?.kind === "api_key" && !state.edit.id;
  return <><div className="dcred-head">API keys</div>
    {!keys.length && !adding && <span className="dsub">Your machines get each key as an env var, for flsh's image, video, 3D and speech tools.</span>}
    {keys.map(credential => <ApiKeyRow key={credential.id} credential={credential} state={state} actions={actions} />)}
    {adding ? <Form state={state} actions={actions} title="New API key">{null}</Form> : unsavedProviders(state).length > 0 && <div className="dline tight wrap dmt6"><Button small onClick={() => actions.edit("api_key")}>+ Add an API key</Button></div>}
  </>;
}
export function CredentialsView({ state, machines, actions }: { state: CredState; machines: Machines; actions: CredentialActions }) {
  if (state.list === undefined) return <SkeletonRows widths={[78, 56]} />;
  if (state.list === null) return <div className="dmuted">Could not load your credentials.</div>;
  const addingForgejo = state.edit?.kind === "forgejo" && !state.edit.id, accounts = githubAccounts(state);
  const canAddGithub = accounts.length > 0 && !(state.edit?.kind === "github" && (!state.edit.id || state.edit.id === "new")) && !state.ghAdd && !state.ghFlow;
  return <div className="dcreds"><div className="dcred-head">Git hosts</div><Github state={state} machines={machines} actions={actions} part="rows" />
    {state.list.filter((credential): credential is Extract<Credential, { kind: "forgejo" }> => credential.kind === "forgejo").map(credential => <Forgejo key={credential.id} credential={credential} state={state} machines={machines} actions={actions} />)}
    {teaOffers(state.cli, state.list).map(server => { const key = `forgejo:${server.url}`; return <GitRow key={key} icon="forgejo" name={hostOf(server.url)} account="teacli on this computer" usable={false} acts={<Use label="Use teacli sign-in" busy={state.cliBusy === key} action={() => actions.useCli(key)} />}><span className="dsub">Signed in with teacli here, not in Beep yet</span>{state.cliError?.key === key && <span className="derr">{state.cliError.message}</span>}</GitRow>; })}
    <Github state={state} machines={machines} actions={actions} part="adding" />
    {addingForgejo && <Form state={state} actions={actions} title="New Forgejo/Gitea server" subtitle="one token per server, teacli on every machine">Make one on your server at <code>/user/settings/applications</code>, with repository and issue access. Or run <code>{"teacli login https://<server> --token <token>"}</code> on this computer and use that sign-in.</Form>}
    {(canAddGithub || !addingForgejo) && <div className="dline tight wrap dmt6">{canAddGithub && <Button small onClick={() => actions.addAccount(true)}>+ Add another GitHub account</Button>}{!addingForgejo && <Button small onClick={() => actions.edit("forgejo")}>+ Add a Forgejo/Gitea server</Button>}</div>}
    <ApiKeys state={state} actions={actions} />
  </div>;
}
