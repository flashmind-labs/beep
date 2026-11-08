// Diagram previews use the same controls on the desktop and phones.
(() => {
  const platform = document.currentScript.dataset.platform;
  const desktop = platform === "desktop";
  const views = desktop ? D : platform === "ios" ? P : R;
  const catalog = desktop ? DESK_SCREENS : platform === "ios" ? IOS_SCREENS : SCREENS;
  const id = desktop ? "dMermaid" : "ccMermaid";
  const source = "flowchart LR\n  A[Write change] --> B[Run checks]\n  B --> C[Review]";
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="160" viewBox="0 0 600 160"><rect width="600" height="160" fill="white"/><g fill="#f3f4f6" stroke="#596477"><rect x="16" y="55" width="160" height="50" rx="4"/><rect x="220" y="55" width="160" height="50" rx="4"/><rect x="424" y="55" width="160" height="50" rx="4"/><path d="M176 80h44m-9-5 9 5-9 5m169-5h44m-9-5 9 5-9 5" fill="none"/></g><g fill="#242b37" text-anchor="middle" font-family="sans-serif" font-size="16"><text x="96" y="86">Write change</text><text x="300" y="86">Run checks</text><text x="504" y="86">Review</text></g></svg>';
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  let expanded = false, code = false, zoom = 1;
  views[id] = () => `<div style="padding:24px;${desktop ? "width:720px;margin:40px auto" : "height:100%;overflow:auto"}"><h3>Explain the review flow</h3><p>Each change passes through checks before review.</p><section class="mermaid-mock${expanded ? " expanded" : ""}" aria-label="Diagram">
    <nav>${expanded ? '<button data-diagram="out" aria-label="Zoom out">-</button><button data-diagram="in" aria-label="Zoom in">+</button><button data-diagram="fit">Fit</button>' : ''}<button data-diagram="expand">${expanded ? "Close" : "Expand"}</button><button data-diagram="source" aria-pressed="${code}">Source</button><button data-diagram="copy-image">Copy image</button><button data-diagram="copy">Copy source</button><button data-diagram="svg">SVG</button><button data-diagram="png">PNG</button></nav>
    <div class="canvas">${code ? `<pre>${esc(source)}</pre>` : `<img src="${url}" alt="Write change, run checks, review" style="width:${zoom * 100}%">`}</div></section><p>Unfinished blocks stay as code. Invalid diagrams show their source and an error.</p></div>`;
  catalog.push({ group: "Conversation", items: [[id, "Mermaid diagram", "Diagram preview and exports"]] });
  const style = document.createElement("style");
  style.textContent = '.mermaid-mock{border:1px solid #7775;border-radius:8px;overflow:hidden}.mermaid-mock nav{display:flex;gap:4px;flex-wrap:wrap;padding:8px}.mermaid-mock button{font:inherit;font-size:14px;padding:8px 12px;border:0;border-radius:4px;cursor:pointer}.mermaid-mock button:focus-visible{outline:2px solid #638ac8}.mermaid-mock button:hover{filter:brightness(.9)}.mermaid-mock button:active{transform:scale(.98)}.mermaid-mock .canvas{overflow:auto;background:white}.mermaid-mock img{display:block;min-width:100%}.mermaid-mock pre{color:#242b37;padding:16px}.mermaid-mock.expanded{position:absolute;inset:0;background:var(--d-bg,var(--m-bg,#181818));z-index:100;display:flex;flex-direction:column}.mermaid-mock.expanded .canvas{flex:1;display:grid;align-items:center}';
  style.textContent += '.diagram-menu{position:fixed;z-index:20000;display:grid;padding:4px;border:1px solid #8885;border-radius:8px;background:#181818;color:white;box-shadow:0 8px 24px #0004}.diagram-menu button{background:transparent;color:inherit;text-align:left;font:14px/20px system-ui;padding:8px 12px;border:0}.diagram-menu button:hover,.diagram-menu button:focus-visible{background:#313131}';
  document.head.append(style);
  async function png() {
    const image = new Image(); image.src = url; await image.decode();
    const canvas = document.createElement("canvas"); canvas.width = 1200; canvas.height = 320;
    canvas.getContext("2d").drawImage(image, 0, 0, 1200, 320);
    return new Promise(resolve => canvas.toBlob(resolve, "image/png"));
  }
  function copyImage() { return navigator.clipboard.write([new ClipboardItem({ "image/png": png() })]); }
  document.addEventListener("contextmenu", e => {
    if (!e.target.closest(".mermaid-mock img")) return;
    e.preventDefault();
    document.querySelector(".diagram-menu")?.remove();
    const menu = document.createElement("div"); menu.className = "diagram-menu"; menu.setAttribute("role", "menu");
    menu.innerHTML = '<button role="menuitem" data-diagram="copy-image">Copy image</button><button role="menuitem" data-diagram="png">Save image to Downloads</button><button role="menuitem" data-diagram="svg">Save as SVG</button><button role="menuitem" data-diagram="copy">Copy source</button>';
    document.body.append(menu);
    menu.style.left = `${Math.min(e.clientX, innerWidth - menu.offsetWidth - 8)}px`;
    menu.style.top = `${Math.min(e.clientY, innerHeight - menu.offsetHeight - 8)}px`;
    menu.querySelector("button").focus();
  });
  document.addEventListener("pointerdown", e => { if (!e.target.closest(".diagram-menu")) document.querySelector(".diagram-menu")?.remove(); });
  document.addEventListener("keydown", e => {
    if (e.key === "Escape") { document.querySelector(".diagram-menu")?.remove(); return; }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && S.screen === id && !code && !getSelection()?.toString()) { e.preventDefault(); void copyImage(); }
  });
  document.addEventListener("click", async e => {
    const action = e.target.closest("[data-diagram]")?.dataset.diagram;
    if (!action) return;
    e.preventDefault(); e.stopImmediatePropagation();
    document.querySelector(".diagram-menu")?.remove();
    if (action === "copy-image") { await copyImage(); return; }
    if (action === "copy") { await navigator.clipboard.writeText(source); return; }
    if (action === "svg" || action === "png") {
      let href = url;
      if (action === "png") {
        const image = new Image(); image.src = url; await image.decode();
        const canvas = document.createElement("canvas"); canvas.width = 1200; canvas.height = 320;
        canvas.getContext("2d").drawImage(image, 0, 0, 1200, 320); href = canvas.toDataURL();
      }
      const link = document.createElement("a"); link.href = href; link.download = `diagram.${action}`; link.click(); return;
    }
    if (action === "expand") expanded = !expanded;
    if (action === "source") code = !code;
    if (action === "in") zoom = Math.min(4, zoom * 1.25);
    if (action === "out") zoom = Math.max(1, zoom / 1.25);
    if (action === "fit") zoom = 1;
    render();
  }, true);
  if (location.hash.slice(1) === id) { S.screen = id; render(); }
})();
