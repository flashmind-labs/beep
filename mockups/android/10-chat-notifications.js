// ---- Chat notifications ----
// Each chat is a conversation notification with its own shortcut, so it can float as a bubble.
// A chat with Beep shows only its latest reply; groups show who said what. Routine results come as Beep in the routine's chat.
R.chatNotifs = () => {
  const st = S.notif;
  const head = (bg, txt) => `<span style="width:36px;height:36px;border-radius:50%;background:${bg};display:grid;place-items:center;font-weight:700;color:var(--m-ink);flex:none">${txt === "b" ? BI(20) : txt}</span>`;
  const conv = (o) => `<div class="notif" style="display:grid;gap:6px">
    <div class="app" style="display:flex;align-items:center;gap:6px"><i>${BI(11)}</i>Beep &middot; ${o.when}
      <span style="margin-left:auto;display:flex;gap:4px"><button class="ib" style="width:28px;height:28px;color:inherit" data-set="notif:bubble" aria-label="Show as a bubble"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="16" cy="8" r="4"/><path d="M4 20h12M4 14h6" stroke-linecap="round"/></svg></button></span></div>
    <div style="display:flex;gap:10px;align-items:flex-start">${head(o.bg, o.ini)}
      <div style="display:grid;gap:4px;min-width:0"><div class="t">${o.title}</div>
        ${o.lines.map(([who, text]) => `<div class="b" style="margin:0"><b>${who}</b> ${text}</div>`).join("")}</div></div>
    <div class="a"><button data-go="${o.go}">Reply</button><button data-toast="Marked as read">Mark as read</button></div></div>`;
  const chat = conv({ when: "now", bg: "var(--m-t1)", ini: "b", title: "Boiler and Saturday weather", go: "chat",
    lines: [["Beep:", "Yes, Thursday between 9 and 12 works for her. Want me to confirm?"]] });
  const group = conv({ when: "4m", bg: "var(--m-t2)", ini: "F", title: "Flat 3B", go: "groupChat",
    lines: [["Marta:", "who's buying bin bags this week?"], ["Beep:", "Pau, it's your turn. I put it on your list."]] });
  const routine = conv({ when: "8:00", bg: "var(--m-t0)", ini: "b", title: "Morning briefing", go: "chat",
    lines: [["Beep:", "Dry and 19°C. Lunch with Sam at 13:00, and the gym is on your way back."]] });
  if (st === "bubble" || st === "open") {
    const bubble = `<button style="all:unset;cursor:pointer;position:absolute;right:12px;top:${st === "open" ? 70 : 220}px;width:56px;height:56px;border-radius:50%;background:var(--m-t1);border:2px solid var(--m-ledge);box-shadow:0 6px 18px #0004;display:grid;place-items:center;font-weight:700;font-size:20px;z-index:3" data-set="notif:${st === "open" ? "bubble" : "open"}" aria-label="Boiler and Saturday weather"><svg width="18" height="18" viewBox="0 0 18 18" fill="currentColor" aria-hidden="true"><circle cx="9" cy="9" r="4"/></svg>
      ${st === "bubble" ? `<span style="position:absolute;right:-2px;top:-2px;width:14px;height:14px;border-radius:50%;background:var(--m-accent);border:2px solid var(--m-page)"></span>` : ""}</button>`;
    const home = `<div style="position:absolute;inset:0;background:linear-gradient(170deg,#2c4a40,#11201b);"></div>
      <div style="position:absolute;left:0;right:0;bottom:30px;display:flex;justify-content:space-around;padding:0 30px">${["#C8674E", "#3B6E8F", "#2F7D5B", "#8A6FD1"].map(c => `<span style="width:48px;height:48px;border-radius:var(--radius);background:${c}"></span>`).join("")}</div>`;
    const panel = st === "open" ? `<div style="position:absolute;left:10px;right:10px;top:136px;bottom:20px;border-radius:var(--radius);overflow:hidden;background:var(--m-page);display:flex;flex-direction:column;box-shadow:0 10px 30px #0006;z-index:2">
      <div class="bar" style="padding:0 12px"><div style="flex:1"><div class="title" style="font-size:15px">Boiler and Saturday weather</div></div>
        <button class="ib" data-go="chat" aria-label="Open in Beep">${extIc}</button></div>
      <div class="body"><div class="user">Did Alex get back about the plumber?</div><div class="reply">Yes, Thursday between 9 and 12 works for her. Want me to confirm?</div></div>
      ${composer()}</div>` : "";
    return `<div style="position:absolute;inset:0;overflow:hidden">${home}${panel}${bubble}</div>`;
  }
  return `<div class="shade"><div class="label" style="color:#5F6662;padding:4px 8px 0">Conversations</div>${chat}${group}${st === "three" ? routine : ""}
    <div class="label" style="color:#5F6662;padding:4px 8px 0">Silent</div>
    <div class="notif" style="opacity:.75;animation:none"><div class="app"><i style="background:#8A6FD1">M</i>Mail &middot; 12m</div><div class="t">Renfe</div><div class="b">Your ticket to Girona</div></div></div>`;
};

// Registered in the index, with iOS and the bar sharing the phone's content.
SCREENS.find(g => g.group === "Chat").items.splice(4, 0, ["products", "Product cards", "ProductCard, ProductRow"]);
SCREENS.find(g => g.group === "Chat").items.push(["chatNotifs", "Chat notifications", "ChatNotifications, bubbles"]);
VARIANTS.products = vs("prod", [["Row", "row"], ["Single", "single"], ["Images loading", "loading"], ["No price", "noprice"]]);
VARIANTS.chatNotifs = vs("notif", [["Shade", "shade"], ["With a routine", "three"], ["Bubble", "bubble"], ["Bubble open", "open"]]);
