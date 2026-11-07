
// Cost and plan usage at the top right of every command center page. Claude reports its 5-hour
// and weekly limits during sessions; Codex's come from its own app-server on a machine. Dollars
// are list-price estimates; Codex reports tokens only.
(() => {
  const st = document.createElement("style");
  st.textContent = ``;
  document.head.append(st);
})();
