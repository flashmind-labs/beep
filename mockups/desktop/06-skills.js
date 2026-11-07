// ---- Skills ----
// How the user, or a group, likes a recurring task done. Beep writes them on its own after a chat has had
// enough turns, or after a turn with many tool calls, and rewrites one when the user corrects it.
// Personal skills load only in the user's chats, a group's only in that group.
Object.assign(S, { skills: "list" });
const skillIc = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/><path d="M19 13l.6 1.4L21 15l-1.4.6L19 17l-.6-1.4L17 15l1.4-.6z"/></svg>';
const SKILLS = [
  { name: "Book a padel court", desc: "Booking a court at the club for an evening game", uses: 6, when: "Used yesterday", by: "Beep", on: true,
    body: ["Open the club's booking page with the browser and sign in with the saved account.", "Look for 19:00 first, then 20:00. Never earlier on weekdays.", "Pick an indoor court when rain is forecast.", "Book for four, then post the court and time in the Padel group."],
    prefs: ["Court 3 or 4 when free", "Pay with the club credit, not the card"] },
  { name: "Weekly spending summary", desc: "Summing up the week's card spending from the bank emails", uses: 3, when: "Used 4 Oct", by: "you", on: true,
    body: ["Search Gmail for the bank's card notifications since Monday.", "Group by shop type: food, transport, home, other.", "Show a short table, totals first, then the three biggest payments."],
    prefs: ["Euros, no decimals", "Leave out transfers between own accounts"] },
  { name: "Plan a weekend trip", desc: "Finding trains and a place to stay for a weekend away", uses: 2, when: "Used 2 Oct", by: "Beep", on: false,
    body: ["Ask where and which weekend if not said.", "Search trains leaving Friday after 17:00 and back Sunday evening.", "Find two or three places to stay near the station, under 120 a night."],
    prefs: ["Window seat", "No hostels"] },
];
const GROUP_SKILLS = [
  { group: "Tokyo trip", name: "Split a dinner bill", desc: "Working out who owes what after a group dinner", uses: 4, when: "Used 5 Oct", by: "Beep", on: true },
];
