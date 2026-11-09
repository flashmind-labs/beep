(() => {
  const { skinned, skinVariants } = WORKSPACE_MOCK;
  // ---- Settings, Appearance: the skin picker ----
  const SK = [["Linear", "#151617"], ["Classic", "#1a2a23"], ["Quiet", "#161b18"], ["GitHub dark", "#161b22"], ["Zed", "#2f343e"], ["Tokyo Night", "#1f2335"], ["Catppuccin Mocha", "#313244"], ["Nord", "#3b4252"], ["Gruvbox", "#3c3836"], ["Rose Pine", "#26233a"], ["Geist", "#000"], ["High contrast", "#000"]];
  const SKL = [["Light", "#f3f3f1"], ["Solarized", "#eee8d5"], ["macOS", "#e9e7ea"], ["Warm", "#f3efe8"]];
  const on = () => S.d.skin === "paper" ? "Light" : "Linear";
  const sw = list => list.map(([n, c]) => `<button class="nb-skin" aria-pressed="${n === on()}" ${n === "Linear" || n === "Light" ? `data-dact="dset:skin:${n === "Light" ? "paper" : ""}" data-dgo="dAppearance"` : `data-toast="Skin: ${n}"`}><i style="background:${c}"></i>${n}</button>`).join("");
  D.dAppearance = () => { const keep = S.d.set; S.d.set = "appearance"; const html = D.dWinSettings(); S.d.set = keep; return skinned(html); };
  VARIANTS.dAppearance = skinVariants;
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dAppearance", "Settings, Appearance", "SettingsSheet, SkinPicker"]);
})();
