import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { ReviewFindings } from "../src/review/review-findings.tsx";
import type { FindingView, ReviewView } from "../src/review/review.ts";

test("review findings retain focus on live updates and dismiss the selected finding", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  const finding: FindingView = { id: 1, area: "src", severity: "high", title: "Example", file: "app.ts", line: 2, why: "Needs attention", verdict: "verified", dropped_why: "", dismissed: false };
  const review: ReviewView = { id: "review", project: "project", branch: "feature", base: "main", head: "abcdef", head_at: 1, started_at: 1, machine_name: "build-box", changed_lines: 2, files: 1, merge: "clean", conflicts: [], pr: null, shards: [], findings: [finding] };
  let selected: FindingView | null = null;
  try {
    const dismiss = (value: FindingView) => { selected = value; };
    const view = render(createElement(ReviewFindings, { review, busy: "", dismiss }));
    const button = view.getByRole("button", { name: "Dismiss" });
    button.focus();
    view.rerender(createElement(ReviewFindings, { review: { ...review, changed_lines: 3 }, busy: "", dismiss }));
    assert.equal(view.getByRole("button", { name: "Dismiss" }), button);
    assert.equal(dom.window.document.activeElement, button);
    fireEvent.click(button);
    assert.equal(selected, finding);
    view.rerender(createElement(ReviewFindings, { review: { ...review, findings: [{ ...finding, dismissed: true }] }, busy: "", dismiss }));
    assert.equal(view.getByRole("button", { name: "Bring back" }), button);
    view.rerender(createElement(ReviewFindings, { review: { ...review, findings: [{ ...finding, verdict: "dropped", dropped_why: "Already handled" }] }, busy: "", dismiss }));
    assert.ok(view.getByText("Not real: Already handled"));
    assert.equal(view.queryByRole("button", { name: "Dismiss" }), null);
    view.rerender(createElement(ReviewFindings, { review: { ...review, findings: [], shards: [{ area: "src", files: 1, lines: 2, agent: "codex", state: "reading", note: "", found: 0 }] }, busy: "", dismiss }));
    assert.ok(view.getByText("Findings show as the reviewers finish."));
    assert.ok(view.getByText("reading"));
    view.rerender(createElement(ReviewFindings, { review: { ...review, findings: [] }, busy: "", dismiss }));
    assert.ok(view.getByText("No findings"));
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
