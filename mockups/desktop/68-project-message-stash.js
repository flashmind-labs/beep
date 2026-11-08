// Browser storage models the desktop's project stash on disk. Each saved
// attachment includes its bytes, so closing its original chat loses nothing.
(() => {
  const key = "beep.mock.project-stashes.v1";
  const fileIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H5v18h14V8zM14 3v5h5M8 12h8M8 16h5"/></svg>';
  const esc = value => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const copy = value => JSON.parse(JSON.stringify(value));
  const makeId = () => Array.from(crypto.getRandomValues(new Uint32Array(4)), n => n.toString(16).padStart(8, "0")).join("");
  const readSaved = text => {
    const raw = JSON.parse(text || "{}");
    if (!raw || Array.isArray(raw) || typeof raw !== "object" || Object.values(raw).some(items => !Array.isArray(items) || items.some(item => typeof item.id !== "string" || typeof item.text !== "string" || !Array.isArray(item.files) || item.files.some(file => typeof file.name !== "string" || typeof file.type !== "string" || typeof file.data !== "string" || !Number.isFinite(file.size))))) throw Error();
    return raw;
  };
  let saved = {}, error = "", loadFailed = false;
  try {
    saved = readSaved(localStorage.getItem(key));
  } catch {
    loadFailed = true;
    error = "Could not read the stash. Reload to try again. Your saved messages have been kept.";
  }
  const drafts = new Map();
  let selectedProject = null, chat = "current", open = false, reading = false;
  try { selectedProject = localStorage.getItem(key + ".project"); } catch {}
  const chooseProject = project => {
    selectedProject = project;
    try { localStorage.setItem(key + ".project", project); } catch {}
  };
  const context = () => document.querySelector("[data-ms-context]");
  const draft = id => {
    if (!drafts.has(id)) drafts.set(id, { text: "", files: [], messages: [] });
    return drafts.get(id);
  };
  const hasMessage = d => !!d.text.trim() || !!d.files.length;
  const bytes = n => n < 1024 ? `${n} B` : `${Math.round(n / 1024)} KB`;
  const save = next => {
    if (loadFailed) return false;
    try { localStorage.setItem(key, JSON.stringify(next)); saved = next; error = ""; return true; }
    catch { error = "Could not save the stash. Your message and files are still here. Free some space and try again."; return false; }
  };
  const entry = d => ({ id: makeId(), text: d.text, files: copy(d.files), at: Date.now() });
  const fileHtml = (file, i, removable = false) => `<span class="ms-file">${file.type.startsWith("image/") && file.data ? `<img src="${esc(file.data)}" alt="">` : fileIcon}<span title="${esc(file.name)}">${esc(file.name)}</span><small>${bytes(file.size)}</small>${removable ? `<button class="ms-file-remove" data-ms-remove-file="${i}" aria-label="Remove ${esc(file.name)}">&times;</button>` : ""}</span>`;
  const filesHtml = (files, removable = false) => `<div class="ms-files">${files.map((f, i) => fileHtml(f, i, removable)).join("")}</div>`;
  const age = at => Date.now() - at < 60000 ? "Just stashed" : new Date(at).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  function controls(project, id) {
    const items = saved[project] || [], d = draft(id);
    if (!items.length) open = false;
    return `<div class="ms-tools">${items.length ? `<button class="ms-button ms-indicator" data-ms-toggle aria-expanded="${open}" aria-controls="project-stash" title="Preview messages stashed in ${esc(project)}">Stash <span class="ms-count">${items.length}</span></button>` : ""}${open ? `<section class="ms-panel" id="project-stash" role="dialog" aria-label="Project stash"><header><div><b>Stash</b><small>${esc(project)} · ${items.length} ${items.length === 1 ? "message" : "messages"}</small></div><button class="ms-button" data-ms-close aria-label="Close stash">&times;</button></header>${items.length ? `<div class="ms-items">${items.map(item => `<article class="ms-item"><p>${esc(item.text || "Attachments only")}</p>${filesHtml(item.files)}<footer><small>${age(item.at)}</small><button class="ms-button" data-ms-delete="${item.id}" aria-label="Delete stashed message">Remove</button><button class="ms-button ms-pop" data-ms-pop="${item.id}" ${reading || loadFailed ? "disabled" : ""}>${hasMessage(d) ? "Stash draft &amp; pop" : "Pop"}</button></footer></article>`).join("")}</div>` : '<p class="ms-empty">No stashed messages in this project.<br>Stash a draft to pick it up in any chat.</p>'}</section>` : ""}</div>`;
  }
  function refresh(focus = false) {
    const root = context();
    if (!root) return;
    const { msProject: project, msContext: id } = root.dataset;
    root.querySelector(".ms-tools").outerHTML = controls(project, id);
    root.querySelector("[data-ms-save]").disabled = !hasMessage(draft(id)) || reading || loadFailed;
    root.querySelector("[data-ms-pop-latest]").disabled = !(saved[project]?.length) || reading || loadFailed;
    root.querySelector(".ms-draft-files").innerHTML = filesHtml(draft(id).files, true);
    root.querySelector(".ms-error").textContent = reading ? "Saving attachment..." : error;
    if (focus) { const input = root.querySelector(".ms-input"); input.value = draft(id).text; input.focus(); }
  }
  window.addEventListener("storage", e => {
    if (e.key !== key) return;
    try { saved = readSaved(e.newValue); loadFailed = false; error = ""; }
    catch { loadFailed = true; error = "Could not read the stash. Reload to try again. Your saved messages have been kept."; }
    refresh();
  });
  function mount(html, isNew) {
    const template = document.createElement("template");
    template.innerHTML = html;
    const body = template.content;
    const box = body.querySelector(".agc-box") || body.querySelector("#agfield");
    if (!box || !box.querySelector(".pr, .txt")) return html;
    const projectPill = body.querySelector('[title="The project this agent works on"], [title="Pick a project to start"]');
    const label = body.querySelector(".agproj");
    const project = selectedProject || (isNew ? (projectPill?.textContent.trim() === "No project" ? null : "acme/widget") : label?.textContent.trim());
    if (!project) return html;
    if (label) label.textContent = project;
    if (projectPill) { const text = [...projectPill.childNodes].find(n => n.nodeType === Node.TEXT_NODE && n.textContent.trim()); if (text) text.textContent = project; }
    const id = `${project}:${isNew && chat === "current" ? "new" : chat}`;
    const d = draft(id);
    const inherited = body.querySelector(".ag-att");
    if (inherited && !inherited.querySelector(".ag-upask, .ag-dropin")) {
      if (!d.files.length) {
        d.files = inherited.querySelector(".ag-ment")
          ? [...inherited.querySelectorAll(".ag-ment")].map(el => {
            const name = el.title.replace("Goes in the message as @", "");
            const data = "Mock attachment for " + name;
            return { name, type: "text/plain", size: data.length, data: "data:text/plain;base64," + btoa(data) };
          })
          : [copy(example().files[0])];
      }
      inherited.remove();
    }
    const root = body.querySelector(".ag-conv") || body.querySelector(".agc");
    root.dataset.msContext = id;
    root.dataset.msProject = project;
    root.dataset.msComposerNew = String(isNew);
    root.dataset.msBusy = String(/working|stopping/i.test(body.querySelector("#agstate")?.textContent || ""));
    root.dataset.msUnavailable = String(/moving|offline/i.test(body.querySelector("#agstate")?.textContent || ""));
    if (isNew) {
      const title = body.querySelector(".agtitle");
      if (title) { title.textContent = "New chat"; title.className = ""; title.removeAttribute("role"); }
    }
    const prior = box.querySelector(".pr, .txt");
    const input = document.createElement("textarea");
    input.className = "ms-input";
    input.rows = 1;
    input.setAttribute("aria-label", "Message");
    input.placeholder = isNew ? "What should we work on?" : "Message Claude Code";
    input.textContent = d.text;
    prior.replaceWith(input);
    box.querySelector(".agc-send")?.setAttribute("data-ms-send", "");
    const files = document.createElement("div");
    files.className = "ms-draft-files";
    files.innerHTML = filesHtml(d.files, true);
    if (isNew) input.before(files); else box.before(files);
    box.querySelector('[aria-label="Attach files"]')?.removeAttribute("data-toast");
    const upload = document.createElement("input");
    upload.type = "file"; upload.multiple = true; upload.hidden = true; upload.dataset.msUpload = "";
    box.append(upload);
    let hints = root.querySelector(".dhints.slim");
    if (!hints) {
      hints = document.createElement("div");
      hints.className = "dhints slim";
      hints.innerHTML = '<span><kbd>⌘M</kbd>model</span><span><kbd>esc</kbd>back</span>';
      root.append(hints);
    }
    let more = hints.querySelector(".ag-shortcuts");
    if (!more) {
      more = document.createElement("details");
      more.className = "ag-shortcuts";
      more.innerHTML = '<summary>Show more</summary><div><span><kbd>⇧↩</kbd>new line</span></div>';
      hints.append(more);
    }
    more.querySelector("div").insertAdjacentHTML("beforeend", `<button class="ms-button" data-ms-save ${!hasMessage(d) || reading || loadFailed ? "disabled" : ""}>Stash draft<kbd>Ctrl S</kbd></button><button class="ms-button" data-ms-pop-latest ${!saved[project]?.length || reading || loadFailed ? "disabled" : ""}>Pop latest<kbd>Ctrl P</kbd></button>`);
    more.insertAdjacentHTML("beforebegin", controls(project, id));
    root.insertAdjacentHTML("beforeend", `<div class="ms-error" role="status">${esc(error)}</div>`);
    const head = body.querySelector(".dhead");
    if (head) head.insertAdjacentHTML("beforeend", `<div class="ms-chat-tools"><select aria-label="Stash project" title="Switch project"><option${project === "acme/widget" ? " selected" : ""}>acme/widget</option><option${project === "acme/relay" ? " selected" : ""}>acme/relay</option>${!["acme/widget", "acme/relay"].includes(project) ? `<option selected>${esc(project)}</option>` : ""}</select><button class="ms-button" data-ms-new>New chat</button></div>`);
    body.querySelectorAll('.agc-menu button').forEach(button => {
      const value = button.textContent.match(/acme\/(widget|relay)/)?.[0];
      if (value) { button.dataset.msProjectChoice = value; button.removeAttribute("data-dact"); }
    });
    const log = body.querySelector(".dcoderlog");
    if (log && d.messages.length) {
      if (d.started) {
        log.innerHTML = "";
        body.querySelectorAll(".agtasks, .dagmain > .dcard").forEach(el => el.remove());
        const title = body.querySelector(".agtitle");
        if (title) { title.className = ""; title.textContent = d.messages[0].text.slice(0, 60) || "Review attached files"; }
      }
      log.insertAdjacentHTML("beforeend", d.messages.map(message => `<div class="dcoderen user"><b>You</b><span>${esc(message.text)}</span>${filesHtml(message.files)}</div>`).join("") + (d.started ? '<div class="ms-sending">Working on your message...</div>' : ""));
    }
    return template.innerHTML;
  }
  const session = D.dAgentSession, newSession = D.dAgentNew;
  D.dAgentNew = () => /^draft/.test(S.d.ags) && S.screen === "dAgentSession" ? newSession() : mount(newSession(), true);
  D.dAgentSession = () => {
    const original = S.d.ags;
    const html = session();
    return mount(html, /^draft/.test(original));
  };
  function stash() {
    const root = context();
    if (!root || reading) return;
    const project = root.dataset.msProject, d = draft(root.dataset.msContext);
    if (!hasMessage(d)) return;
    if (save({ ...saved, [project]: [entry(d), ...(saved[project] || [])] })) {
      d.text = ""; d.files = []; open = false; toast("Message and attachments stashed");
    }
    refresh(true);
  }
  function pop(id) {
    const root = context();
    if (!root || reading) return;
    const project = root.dataset.msProject, d = draft(root.dataset.msContext);
    const item = (saved[project] || []).find(item => item.id === id);
    if (!item) return;
    const remaining = saved[project].filter(item => item.id !== id);
    if (hasMessage(d)) remaining.unshift(entry(d));
    if (save({ ...saved, [project]: remaining })) {
      d.text = item.text; d.files = copy(item.files); open = false; toast("Restored to this chat");
    }
    refresh(true);
  }
  function send() {
    const root = context();
    if (!root || reading) return;
    if (root.dataset.msUnavailable === "true") { error = "This session is unavailable. Keep your draft or stash it for another chat."; refresh(); return; }
    const project = root.dataset.msProject, d = draft(root.dataset.msContext);
    if (!hasMessage(d)) return;
    const isNew = root.dataset.msComposerNew === "true", busy = root.dataset.msBusy === "true";
    d.messages.push({ text: d.text, files: copy(d.files) });
    d.text = ""; d.files = []; open = false; error = "";
    if (isNew) { d.started = true; if (chat === "current") chat = "new"; chooseProject(project); }
    if (S.screen === "dAgentNew") S.screen = "dAgentSession";
    S.d.ags = "working"; render();
    toast(busy ? "Steered into this turn" : "Message sent");
    context()?.querySelector(".ms-input")?.focus();
  }
  const readFile = file => new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve({ name: file.name, type: file.type || "application/octet-stream", size: file.size, data: reader.result });
    reader.onerror = () => reject(Error("Could not read the attachment. Choose it again."));
    reader.readAsDataURL(file);
  });
  async function attach(files) {
    const root = context();
    if (!root || reading) return;
    const d = draft(root.dataset.msContext);
    reading = true; refresh();
    try { d.files.push(...await Promise.all([...files].map(readFile))); error = ""; }
    catch (e) { error = e.message; }
    finally { reading = false; refresh(); }
  }
  document.addEventListener("input", e => {
    if (!e.target.matches(".ms-input")) return;
    const root = e.target.closest("[data-ms-context]");
    draft(root.dataset.msContext).text = e.target.value;
    refresh();
  });
  document.addEventListener("change", e => {
    if (e.target.matches("[data-ms-upload]")) attach(e.target.files);
    if (e.target.matches('[aria-label="Stash project"]')) {
      chooseProject(e.target.value); chat = "new-" + makeId(); open = false;
      S.d.ags = "draft"; render();
    }
  });
  document.addEventListener("click", e => {
    const button = e.target.closest("[data-ms-save], [data-ms-toggle], [data-ms-close], [data-ms-pop], [data-ms-delete], [data-ms-remove-file], [data-ms-new], [data-ms-project-choice], [data-ms-send], [data-ms-pop-latest], [aria-label='Attach files']");
    if (!button || !context()) {
      if (open && !e.target.closest(".ms-panel")) { open = false; refresh(); }
      return;
    }
    e.preventDefault(); e.stopImmediatePropagation();
    const root = context(), project = root.dataset.msProject, d = draft(root.dataset.msContext);
    if (button.hasAttribute("data-ms-send")) send();
    else if (button.hasAttribute("data-ms-save")) stash();
    else if (button.hasAttribute("data-ms-pop-latest")) { const item = saved[project]?.[0]; if (item) { if (hasMessage(d)) { open = true; refresh(); } else pop(item.id); } }
    else if (button.hasAttribute("data-ms-toggle")) { open = !open; refresh(); if (open) context().querySelector("[data-ms-close]")?.focus(); }
    else if (button.hasAttribute("data-ms-close")) { open = false; refresh(); (context().querySelector("[data-ms-toggle]") || context().querySelector(".ms-input")).focus(); }
    else if (button.dataset.msPop) pop(button.dataset.msPop);
    else if (button.dataset.msDelete) { save({ ...saved, [project]: saved[project].filter(item => item.id !== button.dataset.msDelete) }); refresh(); }
    else if (button.dataset.msRemoveFile) { d.files.splice(Number(button.dataset.msRemoveFile), 1); refresh(); }
    else if (button.hasAttribute("data-ms-new") || button.dataset.msProjectChoice) {
      chooseProject(button.dataset.msProjectChoice || project);
      chat = "new-" + makeId(); open = false; S.d.ags = "draft"; render();
    } else root.querySelector("[data-ms-upload]")?.click();
  }, true);
  document.addEventListener("keydown", e => {
    if (e.isComposing || !context()) return;
    if (e.key === "Escape" && open) { e.preventDefault(); e.stopImmediatePropagation(); open = false; refresh(); (context().querySelector("[data-ms-toggle]") || context().querySelector(".ms-input")).focus(); return; }
    if (e.key === "Enter" && !e.shiftKey && e.target.matches(".ms-input")) { e.preventDefault(); e.stopImmediatePropagation(); send(); return; }
    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && ["s", "p"].includes(e.key.toLowerCase()) && e.target.matches(".ms-input")) {
      e.preventDefault(); e.stopImmediatePropagation();
      if (e.key.toLowerCase() === "s") stash();
      else {
        const root = context(), d = draft(root.dataset.msContext), item = saved[root.dataset.msProject]?.[0];
        if (item && !hasMessage(d)) pop(item.id);
        else if (item) { open = true; refresh(); }
        else toast("Stash is empty");
      }
    }
  }, true);
  document.addEventListener("dragover", e => { if (e.target.closest("[data-ms-context]")) e.preventDefault(); });
  document.addEventListener("drop", e => {
    if (!e.target.closest("[data-ms-context]")) return;
    e.preventDefault(); if (e.dataTransfer.files.length) attach(e.dataTransfer.files);
  });
  document.addEventListener("paste", e => {
    if (!e.target.matches(".ms-input")) return;
    if (e.clipboardData.files.length) { e.preventDefault(); attach(e.clipboardData.files); }
  });
  const sampleImage = "data:image/svg+xml;base64," + btoa('<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200"><rect width="320" height="200" fill="#eee"/><rect x="60" y="28" width="200" height="144" rx="8" fill="white"/><text x="80" y="60" font-family="sans-serif" font-size="16">Sign in</text><rect x="80" y="76" width="160" height="24" rx="4" fill="#eee"/><rect x="80" y="112" width="160" height="24" rx="4" fill="#313131"/><text x="126" y="128" font-family="sans-serif" font-size="12" fill="white">Continue</text><text x="80" y="156" font-family="sans-serif" font-size="10" fill="#b33">Check your email address</text></svg>');
  const example = () => ({ text: "Check the login error against this screenshot and the test output. Keep the existing wording.", files: [
    { name: "login-screen.svg", type: "image/svg+xml", size: 684, data: sampleImage },
    { name: "test-output.txt", type: "text/plain", size: 39, data: "data:text/plain;base64," + btoa("Expected: Invalid email\nActual: Try again") },
  ], messages: [] });
  let demo = "";
  function preview(mode) {
    chooseProject(mode === "other" ? "acme/relay" : mode === "empty" ? "acme/new-project" : "acme/widget");
    chat = "demo-" + makeId(); open = ["preview", "other", "empty"].includes(mode); demo = mode;
    S.d.ags = ["compose", "files", "preview"].includes(mode) ? "working" : "draft";
    const d = draft(`${selectedProject}:${chat}`);
    if (mode === "compose" || mode === "files") Object.assign(d, example(), mode === "files" ? { text: "" } : {});
    if (["preview", "new"].includes(mode) && !saved[selectedProject]?.length) {
      save({ ...saved, [selectedProject]: [{ ...entry(example()), at: Date.now() - 240000 }, { ...entry({ text: "Review the mobile error copy before we release.", files: [] }), at: Date.now() - 1500000 }] });
    }
    render();
  }
  VARIANTS.dAgentSession.unshift(...[
    ["Stash: message and files", "compose"], ["Stash: preview", "preview"], ["Stash: pop in a new chat", "new"],
    ["Stash: another project", "other"], ["Stash: files only", "files"], ["Stash: empty", "empty"],
  ].map(([label, mode]) => [label, () => preview(mode), () => demo === mode]));
  window.addEventListener("mockup:save", e => {
    e.detail.projectStash = { selectedProject, chat, open, demo, drafts: [...drafts] };
  });
  window.addEventListener("mockup:restore", e => {
    const state = e.detail.projectStash;
    if (!state) return;
    selectedProject = state.selectedProject;
    chat = state.chat;
    open = state.open;
    demo = state.demo;
    drafts.clear();
    for (const [id, value] of state.drafts) drafts.set(id, value);
  });
  const mode = new URLSearchParams(location.search).get("stash");
  if (window.__mockupRestoring) return;
  if (["compose", "preview", "new", "other", "files", "empty"].includes(mode)) {
    S.screen = "dAgentSession"; preview(mode);
  } else if (location.hash === "#dAgentSession" || ["dAgentSession", "dAgentNew"].includes(S.screen)) {
    if (location.hash === "#dAgentSession") S.screen = "dAgentSession";
    render();
  }
})();
