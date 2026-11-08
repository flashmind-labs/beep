// Double-click the session title to override it. An empty value restores it.
(() => {
  let override = "";
  const original = "Fix the failing login test";
  const base = D.dAgentSession;
  D.dAgentSession = () => {
    const template = document.createElement("template");
    template.innerHTML = base();
    const title = template.content.querySelector(".dhead .tt b");
    if (title) {
      const project = title.querySelector(".agproj");
      const location = title.nextElementSibling;
      if (project && location) location.prepend(project, " @ ");
      title.className = "agtitle";
      title.tabIndex = 0;
      title.setAttribute("role", "button");
      title.title = "Double-click to rename (Enter)";
      title.textContent = override || original;
    }
    return template.innerHTML;
  };
  const draw = () => document.querySelectorAll(".agtitle").forEach(title => { title.textContent = override || original; });
  new MutationObserver(draw).observe(document.getElementById("desk"), { childList: true });
  function edit(title) {
    if (!title || title.querySelector("input")) return;
    const input = document.createElement("input");
    input.className = "agtitle-input";
    input.value = override || original;
    input.maxLength = 200;
    input.setAttribute("aria-label", "Session title");
    let done = false;
    const finish = save => {
      if (done) return;
      done = true;
      if (save) override = input.value.trim();
      title.replaceChildren(override || original);
      title.focus();
    };
    input.addEventListener("blur", () => finish(true));
    input.addEventListener("keydown", e => {
      e.stopPropagation();
      if (e.isComposing) return;
      if (e.key === "Enter" || e.key === "Escape") {
        e.preventDefault(); finish(e.key === "Enter");
      }
    });
    title.replaceChildren(input);
    input.focus(); input.select();
  }
  document.addEventListener("dblclick", e => edit(e.target.closest(".agtitle")));
  document.addEventListener("keydown", e => {
    if (e.target.matches(".agtitle") && e.key === "Enter") { e.preventDefault(); edit(e.target); }
  });
})();
