// Current app entry points stay together; older proposals remain available.
{
  const currentIds = ["dIdeaBoard", "dHome", "dWinProjects", "dWinProject", "dAgents", "dAgentSession", "dTerminals", "dSync", "dWinSettings", "dAddTicket", "dReport"];
  const all = DESK_SCREENS.flatMap(group => group.items);
  const current = currentIds.flatMap(id => {
    const item = all.find(([key]) => key === id);
    return item ? [item] : [];
  });
  for (const group of DESK_SCREENS) group.items = group.items.filter(([id]) => !currentIds.includes(id));
  const older = DESK_SCREENS.filter(group => group.items.length);
  const ideas = older.filter(group => group.group.startsWith("Ideas"));
  const previews = older.filter(group => !ideas.includes(group));
  for (const group of previews) group.group = `Preview: ${group.group}`;
  DESK_SCREENS.splice(0, DESK_SCREENS.length, { group: "Current app", items: current }, ...ideas, ...previews);

  const saved = MOCKUP_SELECTION;
  const requested = location.hash.slice(1);
  if (requested && typeof D[requested] === "function") S.screen = requested;
  else S.screen = saved && typeof D[saved] === "function" ? saved : "dHome";
  render();
}
