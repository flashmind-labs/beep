// The sheets and screens that views ask for with `openScreen`: this opens them, so a view
// never imports the screen it wants.
import { MACHINES } from "../machines/machine-store.ts";
import { landed } from "../project/addflow.ts";
import { screenWanted, type ScreenRequest } from "./nav.ts";
import { openSettings } from "../settings/sheet.ts";
import { state } from "./state.ts";

screenWanted.follow(open);

function open(request: ScreenRequest) {
  switch (request.screen) {
    case "addmachine": return void import("../machines/addmachine.tsx").then(m => m.openAddMachine());
    case "importproject": return importSheet(request.repo, request.machine, request.openAfter);
    case "addproject": return addProject();
    case "teamjoin": return void import("../project/teamjoin.tsx").then(m => m.openTeamJoin(null));
    case "machines": return void import("../machines/machines.tsx").then(m => m.renderMachines());
    case "settings": return openSettings(request.section ?? undefined);
    case "newthread": return void import("../agents/assistant-page.tsx").then(m => m.newThread(request.chat));
  }
}

function importSheet(repo: string | null, machine: string | null, openAfter: boolean) {
  void import("../addproject/importproject.tsx").then(m => m.openImportProject({
    machines: state.home?.machines || [],
    macId: MACHINES.mac?.id || null,
    repo,
    machine,
    onDone: openAfter ? landed : null,
  }));
}

function addProject() {
  void import("../addproject/addproject.tsx").then(m => m.openAddProject({
    machines: (state.home?.machines || []).filter(mac => mac.can_host !== false),
    onImported: landed,
    onChoose: () => void import("../addproject/addfolder.tsx").then(f => f.openAddFolder({
      machines: state.home?.machines || [],
      macId: MACHINES.mac?.id || null,
      onDone: landed,
      onBack: addProject,
    })),
    onClone: repo => importSheet(repo ?? null, null, true),
    onNew: () => void import("../addproject/newproject.tsx").then(n => n.openNewProject({
      machines: state.home?.machines || [],
      onDone: landed,
      onBack: addProject,
      onClone: repo => importSheet(repo, null, true),
    })),
  }));
}
