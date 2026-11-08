// Only this local page receives diagram text. Rendered SVG is displayed as an image.
(() => {
  const $ = id => document.getElementById(id);
  const stage = $("stage"), picture = $("picture"), code = $("code"), status = $("status");
  let source = "", svg = "", imageURL = "", expanded = false, zoom = 1, busy = false;
  let drag = null, queued = null, engine = null, png = "";
  const menu = $("menu");
  function loadEngine() {
    return engine ??= new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = "mermaid.min.js";
      script.onload = resolve; script.onerror = () => reject(new Error("Diagram renderer unavailable."));
      document.head.append(script);
    });
  }
  function send(action, extra = {}) {
    const message = { kind: "beep-diagram", action, ...extra };
    if (window.webkit?.messageHandlers?.diagram) window.webkit.messageHandlers.diagram.postMessage(message);
    else if (window.beepDiagram?.postMessage) window.beepDiagram.postMessage(JSON.stringify(message));
    else if (parent !== window) parent.postMessage(message, "*");
  }
  const resize = () => send("resize", { height: Math.ceil(document.querySelector("main").getBoundingClientRect().height) });
  new ResizeObserver(resize).observe(document.querySelector("main"));
  function encoded(text) {
    const bytes = new TextEncoder().encode(text);
    let binary = "";
    for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
    return btoa(binary);
  }
  function fit() {
    if (!expanded || !picture.naturalWidth) return;
    const width = Math.min(stage.clientWidth, stage.clientHeight * picture.naturalWidth / picture.naturalHeight);
    picture.style.width = `${Math.max(1, width) * zoom}px`;
  }
  addEventListener("resize", fit);
  async function render(input) {
    if (typeof input.source !== "string") return;
    if (busy) { queued = input; return; }
    busy = true; png = "";
    $("copy-image").disabled = true; menu.hidden = true;
    $("svg").disabled = true; $("png").disabled = true;
    source = input.source;
    expanded = input.expanded === true;
    menu.querySelector('[data-image-action="save"]').textContent = input.desktop ? "Save image to Downloads" : "Save image";
    document.body.classList.toggle("expanded", expanded);
    $("expand").textContent = expanded ? "Close" : "Expand";
    code.textContent = source;
    if (input.dark != null) document.documentElement.style.colorScheme = input.dark ? "dark" : "light";
    try {
      if (!source.trim() || source.length > 50000) throw new Error("Diagram is empty or too large.");
      // Configuration stays with the app. A diagram cannot change its security or resource limits.
      if (/%%\s*\{|^\s*---/.test(source)) throw new Error("Diagram configuration is not supported.");
      if (input.failed) throw new Error("Invalid diagram.");
      let image = input.svg;
      if (typeof image !== "string" || image.length > 4000000) {
        await loadEngine();
        mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: "base", look: "classic", htmlLabels: false,
          themeVariables: { primaryColor: "#f3f4f6", primaryTextColor: "#242b37", primaryBorderColor: "#596477", lineColor: "#596477", secondaryColor: "#fff", tertiaryColor: "#f3f4f6" },
          flowchart: { htmlLabels: false }, fontFamily: "sans-serif", maxTextSize: 50000, maxEdges: 500,
          suppressErrorRendering: true, secure: ["secure", "securityLevel", "startOnLoad", "maxTextSize", "maxEdges", "htmlLabels", "theme", "fontFamily", "suppressErrorRendering"] });
        const rendered = await mermaid.render("diagram-render", source);
        image = rendered.svg;
      }
      const doc = new DOMParser().parseFromString(image, "image/svg+xml");
      if (doc.querySelector("parsererror")) throw new Error("Invalid diagram image.");
      // Exports must be self contained too, including when opened outside this page's CSP.
      for (const node of doc.querySelectorAll("script,foreignObject,image,iframe,object,embed,a")) node.remove();
      for (const node of doc.querySelectorAll("*")) {
        for (const attr of [...node.attributes]) {
          if (/^on/i.test(attr.name) || /href$/i.test(attr.name) || /(?:@import|url\(\s*["']?\s*(?!#)[^)]*\))/i.test(attr.value)) node.removeAttribute(attr.name);
        }
      }
      for (const style of doc.querySelectorAll("style")) {
        style.textContent = style.textContent.replace(/@import[^;]*;?|url\(\s*["']?\s*(?!#)[^)]*\)/gi, "");
      }
      const root = doc.documentElement;
      const box = root.getAttribute("viewBox")?.split(/[\s,]+/).map(Number);
      if (!box || box.length !== 4 || !box.every(Number.isFinite) || box[2] <= 0 || box[3] <= 0 || box[2] > 100000 || box[3] > 100000) throw new Error("Invalid diagram size.");
      root.setAttribute("width", String(Math.ceil(box[2])));
      root.setAttribute("height", String(Math.ceil(box[3])));
      root.setAttribute("style", "background: white; color: #333");
      svg = new XMLSerializer().serializeToString(root);
      if (imageURL) URL.revokeObjectURL(imageURL);
      imageURL = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
      picture.src = imageURL;
      picture.alt = source.match(/^\s*accTitle\s*:\s*(.+)$/m)?.[1] || "Mermaid diagram";
      await picture.decode();
      stage.hidden = false; code.hidden = true; status.textContent = "";
      $("source").disabled = false; $("source").setAttribute("aria-pressed", "false");
      $("copy-image").disabled = false;
      $("svg").disabled = false; $("png").disabled = false;
      fit();
      send("rendered", { svg, source });
    } catch (error) {
      svg = ""; stage.hidden = true; code.hidden = false;
      $("source").disabled = true; $("source").setAttribute("aria-pressed", "true");
      status.textContent = error.message === "Diagram configuration is not supported." ? error.message : "Could not render diagram. Source is shown below.";
      send("error", { source });
    } finally {
      busy = false; resize();
      if (queued) { const next = queued; queued = null; void render(next); }
    }
  }
  // Native clients call this after the local page finishes loading. Source never enters HTML.
  window.renderDiagram = render;
  addEventListener("message", event => {
    if (event.source === parent && event.data?.kind === "beep-diagram" && event.data.action === "render") render(event.data);
  });
  $("expand").onclick = () => send(expanded ? "close" : "expand");
  picture.onclick = () => { if (!expanded) send("expand"); };
  $("source").onclick = () => {
    code.hidden = !code.hidden; stage.hidden = !code.hidden;
    $("source").setAttribute("aria-pressed", String(!code.hidden)); resize();
  };
  $("copy").onclick = () => send("copy", { text: source });
  $("svg").onclick = () => { if (svg) send("save", { name: "diagram.svg", mime: "image/svg+xml", data: encoded(svg) }); };
  function pngData() {
    if (png) return png;
    const canvas = document.createElement("canvas");
    const scale = Math.min(2, 4096 / Math.max(picture.naturalWidth, picture.naturalHeight));
    canvas.width = Math.max(1, Math.ceil(picture.naturalWidth * scale));
    canvas.height = Math.max(1, Math.ceil(picture.naturalHeight * scale));
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "white"; ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(picture, 0, 0, canvas.width, canvas.height);
    return png = canvas.toDataURL("image/png").split(",")[1];
  }
  function imageAction(copy) {
    if (!svg || busy) return;
    menu.hidden = true;
    try { send(copy ? "copy-image" : "save", { name: "diagram.png", mime: "image/png", data: pngData() }); }
    catch { status.textContent = copy ? "Could not copy image." : "Could not export PNG. Try SVG instead."; }
  }
  $("copy-image").onclick = () => imageAction(true);
  $("png").onclick = () => imageAction(false);
  stage.addEventListener("contextmenu", event => {
    if (!svg || busy || stage.hidden) return;
    event.preventDefault(); menu.hidden = false;
    menu.style.left = `${Math.max(4, Math.min(event.clientX, innerWidth - menu.offsetWidth - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(event.clientY, innerHeight - menu.offsetHeight - 4))}px`;
    menu.querySelector("button").focus();
  });
  menu.onclick = event => {
    const action = event.target.closest("[data-image-action]")?.dataset.imageAction;
    if (!action) return;
    menu.hidden = true;
    if (action === "copy" || action === "save") imageAction(action === "copy");
    else $(action === "svg" ? "svg" : "copy").click();
  };
  addEventListener("pointerdown", event => { if (!menu.contains(event.target)) menu.hidden = true; });
  addEventListener("blur", () => { menu.hidden = true; });
  const setZoom = value => { zoom = Math.min(8, Math.max(.25, value)); fit(); };
  $("in").onclick = () => setZoom(zoom * 1.25);
  $("out").onclick = () => setZoom(zoom / 1.25);
  $("fit").onclick = () => { setZoom(1); stage.scrollTo(0, 0); };
  stage.addEventListener("wheel", event => {
    if (expanded && (event.ctrlKey || event.metaKey)) { event.preventDefault(); setZoom(zoom * (event.deltaY < 0 ? 1.1 : 1 / 1.1)); }
  }, { passive: false });
  stage.onpointerdown = event => {
    if (!expanded || event.pointerType !== "mouse") return;
    drag = { x: event.clientX, y: event.clientY, left: stage.scrollLeft, top: stage.scrollTop };
    stage.setPointerCapture(event.pointerId);
  };
  stage.onpointermove = event => { if (drag) stage.scrollTo(drag.left + drag.x - event.clientX, drag.top + drag.y - event.clientY); };
  stage.onpointerup = stage.onpointercancel = () => { drag = null; };
  addEventListener("keydown", event => {
    if (!menu.hidden) {
      if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); menu.hidden = true; $("copy-image").focus(); return; }
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const buttons = [...menu.querySelectorAll("button")];
        const at = buttons.indexOf(document.activeElement);
        buttons[(at + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length].focus(); return;
      }
    }
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && !event.isComposing
      && ["c", "s"].includes(event.key.toLowerCase()) && svg && !stage.hidden && !getSelection()?.toString()) {
      event.preventDefault(); event.stopPropagation(); imageAction(event.key.toLowerCase() === "c");
    } else if (expanded && event.key === "Escape") send("close");
  });
  send("ready");
})();
