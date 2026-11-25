import "../../src/bar/bar-app.tsx";
import { renderAgents, drawAgents } from "../../src/agents/agents.ts";
import { loadSessions } from "../../src/agents/sessions.ts";
Reflect.set(window, "openTestAgents", renderAgents);
Reflect.set(window, "refreshTestAgents", async () => { await loadSessions(); drawAgents(false); });
