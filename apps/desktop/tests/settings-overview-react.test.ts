import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentsOverview, InstructionsOverview, MachinesOverview } from "../src/screens/settings/overview-view.tsx";

const noop = () => {};
test("bar overview keeps its loading, empty and navigation states", () => {
  assert.match(renderToStaticMarkup(createElement(MachinesOverview, { machines: undefined, open: noop })), /dskel/);
  assert.equal(renderToStaticMarkup(createElement(MachinesOverview, { machines: null, open: noop })), "");
  assert.match(renderToStaticMarkup(createElement(MachinesOverview, { machines: [], open: noop })), /None linked yet/);
  assert.match(renderToStaticMarkup(createElement(AgentsOverview, { open: noop })), /id="openProviders"/);
  assert.match(renderToStaticMarkup(createElement(InstructionsOverview, { open: noop })), /id="openInstr"/);
});
