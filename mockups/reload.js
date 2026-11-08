// Loaded by the preview server before the screen scripts.
(() => {
  const revision = document.currentScript.dataset.revision;
  const key = "mockup:reload:" + location.pathname;
  let previous = null;
  try {
    const value = JSON.parse(sessionStorage.getItem(key));
    if (value?.url === location.href) previous = value;
  } catch {}
  window.__mockupRestoring = !!previous;

  const selector = el => {
    if (!el || el === document.body) return "body";
    if (el.id) return "#" + CSS.escape(el.id);
    const siblings = [...el.parentElement.children].filter(other => other.tagName === el.tagName);
    return selector(el.parentElement) + " > " + el.tagName.toLowerCase() + ":nth-of-type(" + (siblings.indexOf(el) + 1) + ")";
  };
  const elements = query => [...document.querySelectorAll(query)];
  function save() {
    if (typeof S === "undefined") return false;
    const extra = {};
    window.dispatchEvent(new CustomEvent("mockup:save", { detail: extra }));
    const active = document.activeElement;
    const snapshot = {
      url: location.href, state: S, extra,
      fields: elements('textarea, input:not([type="file"]), select').map(el => ({ selector: selector(el), value: el.value, checked: el.checked })),
      details: elements("details").map(el => ({ selector: selector(el), open: el.open })),
      scroll: elements("body *").filter(el => el.scrollTop || el.scrollLeft).map(el => ({ selector: selector(el), top: el.scrollTop, left: el.scrollLeft })),
      window: [scrollX, scrollY],
      focus: { selector: selector(active), start: active?.selectionStart, end: active?.selectionEnd },
    };
    try { sessionStorage.setItem(key, JSON.stringify(snapshot)); return true; }
    catch { return false; }
  }
  function merge(target, source) {
    for (const [name, value] of Object.entries(source)) {
      if (["__proto__", "constructor", "prototype"].includes(name)) continue;
      if (value && typeof value === "object" && !Array.isArray(value) && target[name] && typeof target[name] === "object" && !Array.isArray(target[name])) merge(target[name], value);
      else target[name] = value;
    }
  }
  function restore() {
    if (!previous || typeof S === "undefined" || typeof render !== "function") return;
    const screens = [typeof D !== "undefined" ? D : {}, typeof P !== "undefined" ? P : {}, typeof R !== "undefined" ? R : {}];
    if (!screens.some(registry => typeof registry[previous.state?.screen] === "function")) return;
    merge(S, previous.state);
    window.dispatchEvent(new CustomEvent("mockup:restore", { detail: previous.extra }));
    render();
    for (const field of previous.fields) {
      const el = document.querySelector(field.selector);
      if (!el || el.matches('input[type="file"]')) continue;
      el.value = field.value;
      if (typeof field.checked === "boolean") el.checked = field.checked;
    }
    for (const item of previous.details) {
      const el = document.querySelector(item.selector);
      if (el) el.open = item.open;
    }
    const focus = document.querySelector(previous.focus.selector);
    focus?.focus({ preventScroll: true });
    if (focus?.setSelectionRange && previous.focus.start != null) {
      try { focus.setSelectionRange(previous.focus.start, previous.focus.end); } catch {}
    }
    requestAnimationFrame(() => requestAnimationFrame(() => {
      for (const item of previous.scroll) {
        const el = document.querySelector(item.selector);
        if (el) { el.scrollTop = item.top; el.scrollLeft = item.left; }
      }
      scrollTo(...previous.window);
    }));
  }
  addEventListener("pagehide", save);
  addEventListener("DOMContentLoaded", () => {
    try { restore(); }
    catch (e) { console.warn("Could not fully restore the mockup state", e); }
    finally {
      window.__mockupRestoring = false;
      try { sessionStorage.removeItem(key); } catch {}
    }
    let candidate = null;
    async function check() {
      try {
        const response = await fetch("/__mockup/revision", { cache: "no-store" });
        if (response.ok) {
          const next = (await response.json()).revision;
          if (typeof next !== "string") throw Error("Invalid revision");
          if (next === revision) candidate = null;
          else if (next === candidate) {
            if (save()) { location.reload(); return; }
          } else candidate = next;
        }
      } catch {}
      setTimeout(check, 1000);
    }
    setTimeout(check, 1000);
  });
})();
