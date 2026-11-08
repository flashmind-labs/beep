// Fabricated history for the interactive desktop mock.
(() => {
  const esc = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const chevron = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m6 4 4 4-4 4"/></svg>';
  const file = (path, removed, added, context = 'export function loginError() {') => ({path, removed, added, context});
  const commits = [
    {id:'f8c21a4', subject:'Keep login errors consistent', author:'Alex Chen', day:'Today', time:'11:42', parents:['a73b0d2'], branch:'login-copy', session:'Shorten the login errors', files:[file('src/auth/login.ts', '  return "That password is incorrect";', '  return "Wrong email or password";'), file('tests/login.test.ts', '  expect(message).toBe("That password is incorrect");', '  expect(message).toBe("Wrong email or password");', 'test("login errors use the same message", () => {')]},
    {id:'a73b0d2', subject:'Cover expired login sessions', author:'Alex Chen', day:'Today', time:'11:18', parents:['bb8123d'], branch:'login-copy', session:'Check the login copy', files:[file('tests/login.test.ts', '  const result = await login(user);', '  const result = await login(user, { expired: true });', 'test("expired sessions ask for login", async () => {')]},
    {id:'bb8123d', subject:'Share the login error message', author:'Alex Chen', day:'Today', time:'10:56', parents:['d9e37c1'], branch:'login-copy', session:'Shorten the login errors', files:[file('src/auth/messages.ts', 'export const loginError = "Login failed";', 'export const loginError = "Wrong email or password";', '// Messages used by the login form')]},
    {id:'d9e37c1', subject:'Merge branch fix-approval', author:'Morgan Lee', day:'Today', time:'10:24', parents:['aa420fe','7de104a'], branch:'main', files:[file('src/approval/strip.ts', '  const threshold = 80;', '  const threshold = 64;', 'export function approvalStrip() {'),file('tests/approval.test.ts', '  expect(canApprove(63)).toBe(true);', '  expect(canApprove(63)).toBe(false);', 'test("approval needs a full swipe", () => {')]},
    {id:'7de104a', subject:'Keep approval within thumb reach', author:'Morgan Lee', day:'Yesterday', time:'18:04', parents:['4c65e10'], branch:'fix-approval', session:'Swipe to approve', files:[file('src/approval/strip.ts', '  const threshold = 80;', '  const threshold = 64;', 'export function approvalStrip() {')]},
    {id:'be122aa', subject:'Bump the sync dependency', author:'Jordan Park', day:'Yesterday', time:'17:38', parents:['aa420fe'], branch:'bump-sync', files:[file('package.json', '    "sync-client": "1.3.1"', '    "sync-client": "1.3.2"', '  "dependencies": {')]},
    {id:'aa420fe', subject:'Show the session loading state', author:'Morgan Lee', day:'Yesterday', time:'16:40', parents:['8b2ad31'], branch:'main', session:'Improve session loading', files:[file('src/sessions/list.ts', '  return rows;', '  return loading ? loadingRows() : rows;', 'export function sessionList() {')]},
    {id:'4c65e10', subject:'Add swipe approval interactions', author:'Morgan Lee', day:'Yesterday', time:'15:12', parents:['8b2ad31'], branch:'fix-approval', session:'Swipe to approve', files:[file('src/approval/strip.ts', '  onClick(approve);', '  onSwipe(approve);', 'export function approvalStrip() {')]},
    {id:'8b2ad31', subject:'Clamp the resize handle', author:'Jordan Park', day:'5 November', time:'14:18', parents:['fc90a12'], branch:'main', files:[file('src/panels/resize.ts', '  width = next;', '  width = Math.max(240, next);', 'export function resize(next: number) {')]},
    {id:'fc90a12', subject:'Add the cache test fixture', author:'Alex Chen', day:'5 November', time:'12:26', parents:['2c01a3b'], branch:'main', files:[file('tests/cache.test.ts', '', '  expect(cache.get("missing")).toBe(null);', 'test("a missing key has no value", () => {')]},
    {id:'2c01a3b', subject:'Read settings once at startup', author:'Jordan Park', day:'4 November', time:'09:34', parents:['31bb9f0'], branch:'main', files:[file('src/settings.ts', '  return readSettings();', '  return cachedSettings;', 'export function settings() {')]},
    {id:'31bb9f0', subject:'Create the project layout', author:'Morgan Lee', day:'3 November', time:'16:12', parents:['10aa55b'], branch:'main', files:[file('src/project/layout.ts', '', 'export const columns = ["navigation", "detail"];', '// Project layout')]},
    {id:'10aa55b', subject:'Initial commit', author:'Morgan Lee', day:'3 November', time:'09:08', parents:[], branch:'main', files:[file('README.md', '', '# Widget', '')]},
  ];
  const branches = [{name:'main',tip:'d9e37c1'},{name:'login-copy',tip:'f8c21a4'},{name:'fix-approval',tip:'7de104a'},{name:'bump-sync',tip:'be122aa'}];
  const worktrees = [{name:'main',path:'/srv/project/widget',session:'Improve session loading',changes:[file('src/settings.ts', '  return cachedSettings;', '  return cachedSettings ?? defaults;', 'export function settings() {')]},{name:'login-copy',path:'/srv/project/widget/.worktrees/login-copy',session:'Shorten the login errors',changes:[file('src/auth/login.ts', '  return "Wrong email or password";', '  return loginError;', 'export function loginError() {'),file('tests/login.test.ts','  expect(message).toBeTruthy();','  expect(message).toBe(loginError);','test("login uses the shared message", () => {')]},{name:'fix-approval',path:'/srv/project/widget/.worktrees/fix-approval',session:'Swipe to approve',changes:[]}];
  const H = {open:new Set(['history','day:Today','commit:f8c21a4','changes:f8c21a4']), commit:'f8c21a4', path:'src/auth/login.ts', worktree:null, tab:'changes', origin:'All history', count:6, query:'', session:null};
  const commit = () => commits.find(c => c.id === H.commit) || commits[0];
  const tree = () => worktrees.find(w => w.name === H.worktree);
  const files = () => tree()?.changes || commit().files;
  const node = (key, label, children, cls = '', attrs = '') => `<details data-gh-node="${esc(key)}"${H.open.has(key) ? ' open' : ''} class="gh-node ${cls}"><summary ${attrs}>${chevron}${label}</summary><div class="gh-children">${children}</div></details>`;
  const fileRows = (items, id, wt = '') => items.map(f => `<button class="gh-file" data-gh-file="${esc(f.path)}" data-gh-commit="${id}"${wt ? ` data-gh-worktree="${esc(wt)}"` : ''}${H.path === f.path && (wt ? H.worktree === wt : !H.worktree && H.commit === id) ? ' aria-current="true"' : ''}><span class="gh-status">${f.removed ? 'M' : 'A'}</span><span>${esc(f.path)}</span></button>`).join('');
  const commitNode = (c, prefix = '') => node(`${prefix}commit:${c.id}`, `<span class="gh-dot${c.parents.length > 1 ? ' merge' : ''}"></span><span class="gh-subject">${esc(c.subject)}</span><code>${c.id}</code>`, node(`${prefix}changes:${c.id}`, `<span>Changes</span><small>${c.files.length}</small>`, fileRows(c.files,c.id)), 'gh-commit', `data-gh-select="${c.id}" data-gh-origin="${esc(prefix ? `Branches / ${prefix.slice(0,-1)}` : 'All history')}"${H.commit === c.id && !H.worktree ? ' aria-current="true"' : ''}`);
  const matching = () => commits.filter(c => !H.query || `${c.id} ${c.subject} ${c.author} ${c.branch} ${c.files.map(f=>f.path).join(' ')}`.toLowerCase().includes(H.query.toLowerCase()));
  const ancestry = id => { const ids = new Set(); const visit = tip => {if(ids.has(tip)) return; ids.add(tip); commits.find(c=>c.id===tip)?.parents.forEach(visit);}; visit(id); return commits.filter(c=>ids.has(c.id)); };
  const navigationBody = () => {
    const found = matching(), visible = H.query ? found : found.slice(0,H.count);
    const days = [...new Set(visible.map(c=>c.day))];
    const history = days.map(day=>node(`day:${day}`, `<span>${day}</span><small>${visible.filter(c=>c.day===day).length}</small>`, visible.filter(c=>c.day===day).map(c=>commitNode(c)).join(''))).join('');
    const more = !H.query && H.count < found.length ? `<button class="gh-more" data-gh-more>Load older commits <span>${Math.min(H.count,found.length)} of ${found.length}</span></button>` : '';
    return node('history', `<b>All history</b><small>${found.length}</small>`, history + more + (!found.length ? '<p class="gh-empty">No commits match your search.</p>' : ''))
      + node('branches', '<b>Branches</b><small>4</small>', branches.map(b=>node(`branch:${b.name}`, `<code>${b.name}</code>${b.name==='main'?'<small>default</small>':''}`, ancestry(b.tip).map(c=>commitNode(c,`${b.name}:`)).join(''), '', `data-gh-branch="${b.name}"`)).join(''))
      + node('worktrees', '<b>Worktrees</b><small>3</small>', worktrees.map(w=>node(`worktree:${w.name}`, `<code>${w.name}</code><small>${w.name==='main'?'main checkout':`${w.changes.length} changed`}</small>`, `<div class="gh-location">build-box<br><code>${w.path}</code></div>`+node(`working:${w.name}`, `<span>Uncommitted changes</span><small>${w.changes.length}</small>`, fileRows(w.changes,'',w.name)||'<p class="gh-empty">Working tree clean.</p>', '', `data-gh-working="${w.name}"`)+node(`sessions:${w.name}`, '<span>Sessions</span><small>1</small>', `<button class="gh-file" data-gh-session="${esc(w.session)}" data-gh-worktree="${w.name}"><span class="gh-session-mark">C</span><span>${w.session}</span></button>`))).join(''));
  };
  window.gitHistoryNavigation = () => `<aside class="pj-list gh-nav" aria-label="Git navigation"><div class="gh-nav-head"><button class="gh-back" data-dact="dset:ixp:strip" data-dgo="dIdeaProject">Project</button><b>Git</b><button class="gh-collapse" data-gh-collapse title="Collapse all">Collapse all</button></div><label class="gh-search"><input type="search" placeholder="Find a commit or file" aria-label="Find a commit or file" value="${esc(H.query)}"></label><div class="gh-tree" id="git-history-tree">${navigationBody()}</div><div class="gh-nav-foot">All branches · build-box</div></aside>`;
  const diff = f => {
    const rows = [...(f.context?[{k:' ',old:18,now:18,text:f.context}]:[]),...(f.removed?[{k:'-',old:f.context?19:1,now:'',text:f.removed}]:[]),{k:'+',old:'',now:f.context?19:1,text:f.added},...(f.context?[{k:' ',old:f.removed?20:19,now:20,text:'}'}]:[])];
    const hunk = `@@ -${f.context?18:f.removed?1:0},${(f.context?2:0)+(f.removed?1:0)} +${f.context?18:1},${f.context?3:1} @@`;
    return `<div class="gh-patch"><div class="gh-file-head"><code>${esc(f.path)}</code><span class="gh-added">+1</span>${f.removed?'<span class="gh-removed">−1</span>':''}</div><div class="gh-hunk">${hunk}</div>${rows.map(r=>`<div class="gh-code-row ${r.k==='+'?'add':r.k==='-'?'del':''}"><span>${r.old}</span><span>${r.now}</span><span>${r.k}</span><code>${esc(r.text)}</code></div>`).join('')}</div>`;
  };
  window.gitHistoryDetail = () => {
    const c=commit(),w=tree(), list=files(); const selected=list.find(f=>f.path===H.path);
    const title=H.session || (w?`Uncommitted changes in ${w.name}`:c.subject);
    const meta=w?`${w.path} · build-box`:`${c.author} · ${c.day.toLowerCase()} at ${c.time}`;
    const tabs=[['changes',`Changes ${list.length}`],['details','Details'],['sessions',`Sessions ${w||c.session?1:0}`]];
    const head=`<header class="gh-detail-head"><div class="gh-breadcrumb">${w?`Worktrees / ${esc(w.name)}`:`${esc(H.origin)} / <code>${c.id}</code>`}</div><div class="gh-title-row"><h3>${esc(title)}</h3><button class="dbtn quiet small" data-gh-review>Review</button><button class="dbtn quiet small" data-gh-copy>Copy ${w?'path':'SHA'}</button></div><p>${esc(meta)}</p></header>`;
    let body;
    if(H.session) body=`<div class="gh-session-view"><div class="gh-session-info">Claude Code <span>ended</span></div><p class="gh-user-message">${esc(H.session)}</p><p>Reviewed the changes and ran the checks for this session.</p><div class="gh-session-result">${w ? (w.changes.length ? "This worktree has uncommitted changes." : "Working tree clean.") : `Tests passed. Changes committed as <button data-gh-parent="${c.id}"><code>${c.id}</code></button>.`}</div><button class="dbtn small" data-gh-tab="changes">View changes</button></div>`;
    else if(H.tab==='details') body=`<div class="gh-facts"><span>${w?'Worktree':'Commit'}</span><code>${esc(w?w.path:c.id)}</code><span>Branch</span><code>${esc(w?w.name:c.branch)}</code>${w?'':`<span>Author</span><span>${c.author}</span><span>Parents</span><span>${c.parents.length?c.parents.map(id=>`<button class="gh-parent" data-gh-parent="${id}"><code>${id}</code></button>`).join(' '):'Initial commit'}</span>`}</div>`;
    else if(H.tab==='sessions') body=`<div class="gh-session-view">${w||c.session?`<button class="gh-session-link" data-gh-session="${esc(w?w.session:c.session)}"><b>${esc(w?w.session:c.session)}</b><span>Claude Code · ${w?'working':'ended'} · build-box</span></button>`:'<p class="gh-empty">No linked session for this commit.</p>'}</div>`;
    else if(H.tab==='review') body=`<div class="gh-session-view"><h4>Review ${w?esc(w.name):c.id}</h4><p>Review the ${w?'uncommitted changes':'selected commit'} with an agent.</p><button class="dbtn pri small" data-gh-run-review>Review with Codex</button></div>`;
    else body=`<div class="gh-diff-toolbar"><span>${selected?'1 file':`${list.length} files`} changed</span><span class="gh-added">+${selected?1:list.length}</span><span class="gh-removed">−${(selected?[selected]:list).filter(f=>f.removed).length}</span><button data-gh-all-files>Show all files</button></div><div class="gh-diff-scroll">${(selected?[selected]:list).map(diff).join('')||'<p class="gh-empty">Working tree clean.</p>'}</div>`;
    return `<section class="gh-detail" id="git-history-detail">${head}<nav class="gh-tabs" aria-label="Commit views">${tabs.map(([key,label])=>`<button data-gh-tab="${key}" aria-current="${H.tab===key&&!H.session}">${label}</button>`).join('')}</nav><div class="gh-detail-body">${body}</div></section>`;
  };
  window.gitTreeMock=()=>`<div class="gh-legacy">${window.gitHistoryNavigation()}${window.gitHistoryDetail()}</div>`;
  const redrawDetail=()=>{const el=document.getElementById('git-history-detail'); if(el)el.outerHTML=window.gitHistoryDetail(); document.querySelectorAll('[data-gh-file]').forEach(b=>b.setAttribute('aria-current',String(b.dataset.ghFile===H.path&&(b.dataset.ghWorktree?b.dataset.ghWorktree===H.worktree:!H.worktree&&b.dataset.ghCommit===H.commit)))); document.querySelectorAll('[data-gh-select]').forEach(b=>b.setAttribute('aria-current',String(b.dataset.ghSelect===H.commit&&!H.worktree)));};
  const redrawTree=()=>{const el=document.getElementById('git-history-tree');if(el)el.innerHTML=navigationBody();};
  const choose=(id, origin='All history')=>{H.origin=origin;H.commit=id;H.worktree=null;H.session=null;H.tab='changes';H.path=null;redrawDetail();};
  document.addEventListener('toggle',e=>{const key=e.target.dataset?.ghNode;if(!key)return;e.target.open?H.open.add(key):H.open.delete(key);},true);
  document.addEventListener('input',e=>{if(!e.target.matches('.gh-search input'))return;H.query=e.target.value;H.open.add('history');matching().forEach(c=>H.open.add(`day:${c.day}`));redrawTree();});
  document.addEventListener('click',e=>{
    const b=e.target.closest('button,summary');if(!b)return;
    if(b.hasAttribute('data-gh-select'))choose(b.dataset.ghSelect,b.dataset.ghOrigin);
    if(b.hasAttribute('data-gh-branch'))choose(branches.find(branch=>branch.name===b.dataset.ghBranch).tip,`Branches / ${b.dataset.ghBranch}`);
    if(b.hasAttribute('data-gh-working')){H.worktree=b.dataset.ghWorking;H.path=null;H.session=null;H.tab='changes';redrawDetail();}
    if(b.hasAttribute('data-gh-file')){H.worktree=b.dataset.ghWorktree||null;if(b.dataset.ghCommit)H.commit=b.dataset.ghCommit;H.path=b.dataset.ghFile;H.tab='changes';H.session=null;redrawDetail();}
    if(b.hasAttribute('data-gh-tab')){H.tab=b.dataset.ghTab;H.session=null;redrawDetail();}
    if(b.hasAttribute('data-gh-more')){H.count+=6;redrawTree();}
    if(b.hasAttribute('data-gh-collapse')){H.open.clear();document.querySelectorAll('[data-gh-node]').forEach(d=>d.open=false);}
    if(b.hasAttribute('data-gh-all-files')){H.path=null;redrawDetail();}
    if(b.hasAttribute('data-gh-session')){H.worktree=b.dataset.ghWorktree||H.worktree;H.session=b.dataset.ghSession;redrawDetail();}
    if(b.hasAttribute('data-gh-review')){H.tab='review';H.session=null;redrawDetail();}
    if(b.hasAttribute('data-gh-run-review')){document.querySelector('.gh-detail-body').innerHTML='<div class="gh-session-view"><h4>Review preview</h4><p>The app will start an agent review of this selection.</p></div>';}
    if(b.hasAttribute('data-gh-copy')){navigator.clipboard?.writeText(tree()?.path||commit().id).then(()=>{b.textContent='Copied';}).catch(()=>{});}
    if(b.hasAttribute('data-gh-parent')){const id=b.dataset.ghParent;H.count=commits.length;H.open.add('history');H.open.add(`day:${commits.find(c=>c.id===id).day}`);H.open.add(`commit:${id}`);choose(id);redrawTree();}
  });
  if(['git-tree','git-history'].includes(location.hash.slice(1))){S.screen='dIdeaProject';S.d.ixp='branches';}
  render();
})();
