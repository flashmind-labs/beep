
// The machines icon.
const I = { server: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>' };

// Settings, This Mac: the switch, where the link stands, and the two permissions desktop control needs.
function dThisMac() {
  const m = S.d.mac || "off", on = m !== "off";
  const sub = { off: "Beep can see the screen, click, type and run commands here when you ask",
    connecting: "Connecting…",
    failed: "Couldn't connect: beep-node did not start",
    perm: "Connected. Allow both below",
    ready: "Connected" }[m];
  const perm = (name, why, ok) => `<div class="dtoggle"><span><b>${name}</b><small>${why}</small></span>${ok
    ? '<span class="dtag ok">Allowed</span>'
    : `<button class="dbtn" data-toast="Opens System Settings, Privacy and Security, ${name}">Allow…</button>`}</div>`;
  const rows = on ? perm("Screen Recording", "To see the screen", m === "ready") + perm("Accessibility", "To click, type and read app controls", m === "ready") : "";
  return `<div class="dsec flat" style="padding-top:6px">This Mac</div>
        <div class="dtoggle"><span><b>Let Beep use this Mac</b><small>${sub}</small></span><button class="dswitch" role="switch" aria-checked="${on}" aria-label="Let Beep use this Mac" data-dact="mac"></button></div>${rows}`;
}

const S = {
  screen: "dHome",
  d: { sel: 0, set: "general", update: false },
};

const VARIANTS = {
  dTray: [
    ["Menu", () => { S.d.update = false; render(); }, () => !S.d.update],
    ["Update ready", () => { S.d.update = true; render(); }, () => S.d.update],
  ],
};

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const pad = n => String(n).padStart(2, "0");
// Variants that set one key of S.d: [[label, value], ...].
const dv = (key, list) => list.map(([label, val]) => [label, () => { S.d[key] = val; render(); }, () => S.d[key] === val]);

// Shared desktop action buttons use the dbtn styles and their standard variants.
function desktopButton(label, cls = "", attributes = {}, content = null) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = cls ? `dbtn ${cls}` : "dbtn";
  button.textContent = label;

  if (content !== null) button.innerHTML = content;

  for (const [name, value] of Object.entries(attributes)) {
    if (value === false || value === null) continue;
    button.setAttribute(name, value === true ? "" : String(value));
  }

  return button.outerHTML;
}
