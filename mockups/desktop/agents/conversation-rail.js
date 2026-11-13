// ---- The conversation's scrollbar (#620) ----
// Follows ConversationRail in agents/session-frame.tsx: where the view is, and a tick at each
// message the user sent. A tick jumps to its message and shows it beside the tick on hover.
// A conversation that fits has no scrollbar.
(() => {
  const PROMPT = ".dcoderen.user";

  const baseRender = render;
  render = function () {
    baseRender();
    const body = document.querySelector("#desk .ag-conv > #agbody");
    if (body) place(body);
  };

  function place(body) {
    const total = body.scrollHeight, shown = body.clientHeight;
    if (S.d.scrollbarOff || total <= shown + 1) return;
    const rail = document.createElement("div");
    rail.className = "agrail";
    body.parentElement.style.position = "relative";
    body.after(rail);

    const draw = () => {
      const height = rail.clientHeight, scale = height / total, origin = body.getBoundingClientRect().top - body.scrollTop;
      const ticks = (S.d.promptMarksOff ? [] : [...body.querySelectorAll(PROMPT)]).map(node => ({ node, top: Math.min(height - 2, (node.getBoundingClientRect().top - origin) * scale) }));
      rail.innerHTML = `<div class="th" style="top:${body.scrollTop * scale}px;height:${Math.max(24, shown * scale)}px"></div>`;

      for (const { node, top } of ticks) {
        const text = node.textContent.trim().replace(/^You\s*/, "");
        const tick = document.createElement("button");
        tick.className = "tk";
        tick.style.top = `${top}px`;
        tick.setAttribute("aria-label", `Jump to your message: ${text.slice(0, 80)}`);
        tick.onclick = () => node.scrollIntoView({ block: "start" });
        tick.onpointerenter = () => { rail.querySelector(".agpeek")?.remove(); rail.insertAdjacentHTML("beforeend", `<div class="agpeek" style="top:${top}px" aria-hidden="true"></div>`); rail.lastElementChild.textContent = text; };
        tick.onpointerleave = () => rail.querySelector(".agpeek")?.remove();
        rail.append(tick);
      }
    };
    draw();
    body.addEventListener("scroll", draw, { passive: true });
  }

  // Plays the hover once the log has settled at its end: the latest tick shows its message.
  VARIANTS.dAgentSession.push(["Scrollbar: your message beside a tick", () => { S.d.ags = "working"; render(); setTimeout(() => document.querySelector("#desk .agrail .tk:last-of-type")?.onpointerenter(), 60); }, () => false]);
})();
