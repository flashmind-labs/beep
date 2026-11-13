// The artifacts pane in a session folds and unfolds its preview.
document.getElementById("desk").addEventListener("click", e => {
  if (!e.target.closest("[data-artfold]")) return;
  S.d.artfold = !S.d.artfold; render();
});
