// ---- Research workspace ----
// A research runs on the server: a lead plans it, scouts research in parallel, a critic challenges them and a
// writer drafts the paper. Each agent can run on its own model. The main chat only gets the plan, a rare
// question and the result; the agents' threads and their shared room live quietly in the panel on the right.
document.head.insertAdjacentHTML("beforeend", `<style>
/* The message field runs under all three columns; dragging its top edge makes it taller. */
.rav { width: 20px; height: 20px; border-radius: 6px; display: grid; place-items: center; font-size: 10px; font-weight: 700; color: var(--d-ink); flex: none }
/* On a phone the window keeps its desktop layout and its panes scroll sideways. */
</style>`);
const RTOPIC = "Will solid-state batteries be in mass-market EVs by 2028?";
const RAGENTS = [
  { id: "lead", ini: "L", name: "Lead", role: "Plans and coordinates", model: "Claude Sonnet 5.5", c: "var(--m-accent)" },
  { id: "a", ini: "1", name: "Scout 1", role: "Manufacturing and yields", model: "DeepSeek V4.1 Flash", c: "#5CC49C" },
  { id: "b", ini: "2", name: "Scout 2", role: "Who ships what, and when", model: "GLM 5.3 Flash", c: "#8FB8E8" },
  { id: "c", ini: "3", name: "Scout 3", role: "Cost per kWh", model: "DeepSeek V4.1 Flash", c: "#E8C46D" },
  // A follow-up round's scout: only listed once the user asked the follow-up.
  { id: "d", ini: "4", name: "Scout 4", role: "What gets a pack under 30,000 EUR cars", model: "DeepSeek V4.1 Flash", c: "#7FD1C8", round: 2 },
  { id: "critic", ini: "R", name: "Critic", role: "Challenges weak claims", model: "GPT-6 Luna", c: "#C59AE8" },
  { id: "writer", ini: "W", name: "Writer", role: "Drafts the paper", model: "Claude Haiku 5.5", c: "#E4EEE8" },
];
const ragent = id => RAGENTS.find(a => a.id === id);
// What each agent is doing, by research state. A blank line means it has not started.
const RDOING = {
  plan: { lead: ["Waiting for your go", "wait"] },
  running: { lead: ["Watching the room", "run"], a: ["Reading a pilot-line yield report", "run"], b: ["Comparing announced start dates", "run"], c: ["Done: 6 notes, 4 sources", "ok"], critic: ["Starts after the scouts", ""], writer: ["Starts after review", ""] },
  // Round 2: the round 1 scouts are done, the new one works, the critic and writer come after.
  followup: { lead: ["Coordinating round 2", "run"], a: ["Done: 9 notes, 5 sources", "ok"], b: ["Done: 7 notes, 6 sources", "ok"], c: ["Done: 6 notes, 4 sources", "ok"], d: ["Reading a cell cost curve", "run"], critic: ["Checks the new notes next", ""], writer: ["Revises the paper after review", ""] },
  done: { lead: ["Posted the result", "ok"], a: ["Done: 9 notes, 5 sources", "ok"], b: ["Done: 7 notes, 6 sources", "ok"], c: ["Done: 6 notes, 4 sources", "ok"], critic: ["Done: 4 challenges, 3 settled", "ok"], writer: ["Done: draft v2, 2,400 words", "ok"] },
};
const RSOURCES = [
  ["Pilot line update, Q3 2026", "QuantumScape investor letter", "Cycle yield on the Cobra line reached 82% of target."],
  ["Toyota battery roadmap briefing", "Toyota press release, 2026", "Limited production in 2027 to 2028, first in hybrids."],
  ["Solid-state cell cost outlook", "BloombergNEF, 2026", "Pack cost stays 1.8x to 2.4x lithium-ion through 2030."],
  ["Sulfide electrolyte scale-up", "Nature Energy, 2026", "Dry-room requirements add about 30% to plant capex."],
  ["Samsung SDI 900 Wh/L cell", "Samsung SDI, 2026", "Mass production targeted for 2027, premium segment."],
];
const RNOTES = [
  ["a", "Yields on pilot lines sit between 60% and 82% of target. Nobody reports automotive-grade yields yet.", "1, 4"],
  ["b", "Toyota, Samsung SDI and QuantumScape name 2027 to 2028 for first cars, all in premium or hybrid models.", "2, 5"],
  ["c", "Pack cost stays about twice lithium-ion through 2030 in every forecast found.", "3"],
  ["b", "No maker has announced a sub-40,000 EUR model with solid-state cells.", ""],
];
