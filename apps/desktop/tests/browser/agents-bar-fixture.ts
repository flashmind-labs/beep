import "../../src/shell/bar/bar-app.tsx";
import { renderAgents, drawAgents } from "../../src/parts/session/agents-list.ts";
import { loadSessions } from "../../src/stores/agents/sessions.ts";
Reflect.set(window, "openTestAgents", renderAgents);
Reflect.set(window, "refreshTestAgents", async () => { await loadSessions(); drawAgents(false); });
