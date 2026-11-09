// project/split.ts keeps past sessions and custom agents in separate tabs.
function projectSessionTabs(harness, button) {
  const tab = S.d.projectSessionTab || 'sessions';
  const tabs = `<div class="cc-tabs pj-tabs" role="tablist" aria-label="Project sessions">${[['sessions', 'Sessions', 2], ['agents', 'Agents', 1]].map(([key, label, count]) => `<button role="tab" aria-selected="${tab === key}" data-dact="dset:projectSessionTab:${key}" data-dgo="dWinProject">${label}<span class="n">${count}</span></button>`).join('')}</div>`;
  const sessions = [
    ['claude', 'Retry uploads when the token expires', 'build-box', '2 h'],
    ['codex', 'Bump the toolchain', 'studio', '1 d'],
  ].map(([agent, title, machine, when]) => `<div class="pv-row" data-dgo="dAgentSession"><span class="t">${title}</span><span class="m">${harness(agent)}<span>ended</span><span>${machine}</span><span>widget</span><span>${when}</span></span></div>`).join('');
  const agents = `<div class="cc-sec"><h4></h4>${button('Reload', 'quiet small')}</div><div class="cc-panel"><div class="pj-task ca-row"><span class="t">${harness('claude')}<b>reviewer</b><span class="ca-desc">Reads a diff and lists what is wrong</span></span><span class="m"><span>Default</span><code>.claude/agents/reviewer.md</code></span><span class="acts">${button('Start', 'small')}</span></div></div>`;
  return tabs + (tab === 'agents' ? agents : `<div class="cc-panel">${sessions}</div>`);
}
