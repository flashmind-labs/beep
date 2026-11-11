// Diagram previews use the same controls on the desktop and phones.
// The diagram is drawn in the chat's own colors: no white card, actors once at the top,
// a quiet header with Source, Copy, Expand and an Export menu.
(() => {
  const platform = document.currentScript.dataset.platform;
  const desktop = platform === "desktop";
  const views = desktop ? D : platform === "ios" ? P : R;
  const catalog = desktop ? DESK_SCREENS : platform === "ios" ? IOS_SCREENS : SCREENS;
  const id = desktop ? "dMermaid" : "ccMermaid";
  const source = `sequenceDiagram
  participant Desktop
  participant Phone
  participant Node
  Desktop->>Phone: One-use pairing ticket
  Phone->>Desktop: Join over iroh
  Desktop->>Phone: Signed membership and mesh state
  Desktop->>Node: Add node with a ticket
  par Mesh peers
    Desktop<<->>Node: Exchange membership and state
    Phone<<->>Node: Exchange state and requests
    Desktop<<->>Phone: Exchange membership and state
  end`;
  const colors = { "--dg-fg": "var(--d-fg, #e3e4e6)", "--dg-muted": "var(--d-muted, #8a8f98)", "--dg-line": "var(--d-border-hi, #34363c)",
    "--dg-box": "var(--d-elev, #1c1d1f)", "--dg-bg": "var(--d-surface, #151617)", "--dg-accent": "var(--d-accent, #5e6ad2)" };
  let expanded = false, code = false, zoom = 1, exportOpen = false;

  views[id] = () => `<div style="padding:24px;${desktop ? "width:720px;margin:40px auto" : "height:100%;overflow:auto"}"><h3>How a phone joins the mesh</h3><p>The desktop hands the phone a ticket, then every machine talks to every other directly.</p>
    <section class="mermaid-mock${expanded ? " expanded" : ""}" aria-label="Diagram">
      <header>
        <span class="kind">Sequence diagram</span>
        ${expanded ? '<span class="zoom"><button data-diagram="out" aria-label="Zoom out">-</button><button data-diagram="fit">' + Math.round(zoom * 100) + '%</button><button data-diagram="in" aria-label="Zoom in">+</button></span>' : ""}
        <button data-diagram="source" aria-pressed="${code}">${code ? "Diagram" : "Source"}</button>
        <button data-diagram="copy">Copy</button>
        <span class="export"><button data-diagram="export" aria-haspopup="menu" aria-expanded="${exportOpen}">Export ${chevron}</button>${exportOpen ? exportMenu() : ""}</span>
        <button data-diagram="expand" aria-label="${expanded ? "Close" : "Expand"}">${expanded ? "Close" : expandIcon}</button>
      </header>
      <div class="canvas">${code ? `<pre>${esc(source)}</pre>` : drawing()}</div>
    </section>
    <p>The phone ticket expires after five minutes. Once paired, the phone reaches nodes without the desktop running.</p></div>`;
  catalog.push({ group: "Conversation", items: [[id, "Mermaid diagram", "Diagram preview and exports"]] });

  const style = document.createElement("style");
  style.textContent = `
.mermaid-mock{${Object.entries(colors).map(([k, v]) => `${k}:${v}`).join(";")};margin:16px 0;border:1px solid var(--d-border,#222326);border-radius:8px;background:var(--dg-bg);overflow:hidden}
.mermaid-mock header{display:flex;align-items:center;gap:2px;padding:4px 4px 4px 12px;border-bottom:1px solid var(--d-border,#222326)}
.mermaid-mock .kind{flex:1;font-size:12px;line-height:16px;color:var(--dg-muted)}
.mermaid-mock button{display:inline-flex;align-items:center;gap:4px;font:inherit;font-size:12px;line-height:16px;padding:4px 8px;border:0;border-radius:4px;background:transparent;color:var(--dg-muted);cursor:pointer}
.mermaid-mock button:hover,.mermaid-mock button[aria-pressed="true"],.mermaid-mock button[aria-expanded="true"]{background:var(--dg-box);color:var(--dg-fg)}
.mermaid-mock button:focus-visible{outline:2px solid var(--dg-accent);outline-offset:-2px}
.mermaid-mock .zoom{display:inline-flex;margin-right:8px}.mermaid-mock .zoom button{min-width:28px;justify-content:center;font-variant-numeric:tabular-nums}
.mermaid-mock .export{position:relative}
.mermaid-mock .canvas{overflow:auto;padding:16px;cursor:zoom-in}
.mermaid-mock svg.dg{display:block;margin:0 auto;max-width:none;font:13px/16px system-ui,sans-serif}
.mermaid-mock pre{margin:0;color:var(--dg-fg);font:12px/18px ui-monospace,monospace;cursor:text}
.mermaid-mock.expanded{position:absolute;inset:0;margin:0;border:0;border-radius:0;z-index:100;display:flex;flex-direction:column;background:var(--d-ink,#0f1011)}
.mermaid-mock.expanded .canvas{flex:1;display:grid;place-items:center;cursor:grab}
.diagram-menu{position:absolute;right:0;top:calc(100% + 4px);z-index:20000;min-width:160px;display:grid;padding:4px;border:1px solid var(--d-border-hi,#34363c);border-radius:8px;background:var(--d-elev,#1c1d1f);box-shadow:0 8px 24px #0006}
.diagram-menu.at-pointer{position:fixed;right:auto;top:auto}
.diagram-menu button{justify-content:flex-start;padding:8px;color:var(--d-fg,#e3e4e6);font-size:13px}`;
  document.head.append(style);

  // Mermaid's sequence layout with mirrorActors off, drawn with the chat's colors.
  function drawing() {
    const w = 600, h = 372, xs = [80, 300, 520], names = ["Desktop", "Phone", "Node"];
    const actor = (x, name) => `<rect x="${x - 56}" y="8" width="112" height="32" rx="6" fill="var(--dg-box)" stroke="var(--dg-line)"/><text x="${x}" y="28" fill="var(--dg-fg)" font-weight="600">${name}</text><path d="M${x} 40V${h - 8}" stroke="var(--dg-line)" stroke-dasharray="3 4"/>`;
    const msg = (y, a, b, label, both = false) => `<path d="M${xs[a]} ${y}H${xs[b]}" stroke="var(--dg-fg)" stroke-width="1.25" marker-end="url(#dg-head)"${both ? ' marker-start="url(#dg-head)"' : ""}/><text x="${(xs[a] + xs[b]) / 2}" y="${y - 8}" fill="var(--dg-fg)" class="halo">${label}</text>`;
    return `<svg class="dg" xmlns="http://www.w3.org/2000/svg" width="${w * zoom}" height="${h * zoom}" viewBox="0 0 ${w} ${h}" text-anchor="middle" role="img" aria-label="Desktop gives the phone a pairing ticket, then desktop, phone and node exchange state directly">
      <defs><marker id="dg-head" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0 0L8 4L0 8z" fill="var(--dg-fg)"/></marker></defs>
      <style>.halo{paint-order:stroke;stroke:var(--dg-bg);stroke-width:6px;stroke-linejoin:round}</style>
      <rect x="20" y="212" width="${w - 40}" height="148" rx="6" fill="var(--dg-accent)" fill-opacity=".08" stroke="var(--dg-accent)" stroke-opacity=".5"/>
      <text x="32" y="230" text-anchor="start" fill="var(--dg-accent)" font-size="12" font-weight="600">par</text><text x="60" y="230" text-anchor="start" fill="var(--dg-muted)" font-size="12" class="halo">Mesh peers</text>
      ${xs.map((x, i) => actor(x, names[i])).join("")}
      ${msg(76, 0, 1, "One-use pairing ticket")}${msg(116, 1, 0, "Join over iroh")}${msg(156, 0, 1, "Signed membership and mesh state")}${msg(196, 0, 2, "Add node with a ticket")}
      ${msg(268, 0, 2, "Exchange membership and state", true)}${msg(304, 1, 2, "Exchange state and requests", true)}${msg(340, 0, 1, "Exchange membership and state", true)}
    </svg>`;
  }
  function exportMenu(atPointer = false) {
    return `<div class="diagram-menu${atPointer ? " at-pointer" : ""}" role="menu"><button role="menuitem" data-diagram="copy-image">Copy image</button><button role="menuitem" data-diagram="png">Save PNG</button><button role="menuitem" data-diagram="svg">Save SVG</button><button role="menuitem" data-diagram="copy">Copy source</button></div>`;
  }
  const chevron = '<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 4l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>';
  const expandIcon = '<svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M8 2h4v4M6 12H2V8M12 2L8 6M2 12l4-4" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>';

  // Exports carry the resolved colors, so the file looks like the chat without its stylesheet.
  function svgText() {
    const live = document.querySelector(".mermaid-mock svg.dg");
    const holder = document.createElement("div"); holder.innerHTML = drawing();
    const svg = holder.firstElementChild, computed = getComputedStyle(live ?? document.querySelector(".mermaid-mock") ?? document.body);
    svg.setAttribute("style", Object.keys(colors).map(k => `${k}:${computed.getPropertyValue(k).trim()}`).join(";") + ";font:13px system-ui,sans-serif");
    svg.insertAdjacentHTML("afterbegin", `<rect width="100%" height="100%" fill="var(--dg-bg)"/>`);
    return svg.outerHTML;
  }
  async function png() {
    const image = new Image(); image.src = URL.createObjectURL(new Blob([svgText()], { type: "image/svg+xml" })); await image.decode();
    const canvas = document.createElement("canvas"); canvas.width = image.width * 2; canvas.height = image.height * 2;
    canvas.getContext("2d").drawImage(image, 0, 0, canvas.width, canvas.height);
    return new Promise(resolve => canvas.toBlob(resolve, "image/png"));
  }
  function copyImage() { return navigator.clipboard.write([new ClipboardItem({ "image/png": png() })]); }
  function download(blob, name) { const link = document.createElement("a"); link.href = URL.createObjectURL(blob); link.download = name; link.click(); }

  document.addEventListener("contextmenu", e => {
    if (!e.target.closest(".mermaid-mock svg.dg")) return;
    e.preventDefault();
    document.querySelector(".diagram-menu.at-pointer")?.remove();
    document.body.insertAdjacentHTML("beforeend", exportMenu(true));
    const menu = document.querySelector(".diagram-menu.at-pointer");
    menu.style.left = `${Math.min(e.clientX, innerWidth - menu.offsetWidth - 8)}px`;
    menu.style.top = `${Math.min(e.clientY, innerHeight - menu.offsetHeight - 8)}px`;
    menu.querySelector("button").focus();
  });
  document.addEventListener("pointerdown", e => {
    if (e.target.closest(".diagram-menu, [data-diagram=export]")) return;
    document.querySelector(".diagram-menu.at-pointer")?.remove();
    if (exportOpen) { exportOpen = false; render(); }
  });
  document.addEventListener("keydown", e => {
    if (e.key === "Escape") {
      document.querySelector(".diagram-menu.at-pointer")?.remove();
      if (exportOpen) exportOpen = false; else if (expanded) expanded = false; else return;
      render(); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && S.screen === id && !code && !getSelection()?.toString()) { e.preventDefault(); void copyImage(); }
  });
  document.addEventListener("click", async e => {
    const target = e.target.closest("[data-diagram]") ?? (e.target.closest(".mermaid-mock:not(.expanded) svg.dg") ? { dataset: { diagram: "expand" } } : null);
    const action = target?.dataset.diagram;
    if (!action) return;

    e.preventDefault(); e.stopImmediatePropagation();
    document.querySelector(".diagram-menu.at-pointer")?.remove();
    exportOpen = action === "export" ? !exportOpen : false;

    if (action === "copy-image") await copyImage();
    else if (action === "copy") await navigator.clipboard.writeText(source);
    else if (action === "svg") download(new Blob([svgText()], { type: "image/svg+xml" }), "diagram.svg");
    else if (action === "png") download(await png(), "diagram.png");
    else if (action === "expand") { expanded = !expanded; zoom = 1; }
    else if (action === "source") code = !code;
    else if (action === "in") zoom = Math.min(4, zoom * 1.25);
    else if (action === "out") zoom = Math.max(0.5, zoom / 1.25);
    else if (action === "fit") zoom = 1;

    render();
  }, true);
  if (location.hash.slice(1) === id) { S.screen = id; render(); }
})();
