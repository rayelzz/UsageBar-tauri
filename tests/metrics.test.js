import assert from "node:assert/strict";
import test from "node:test";

import { chooseCompactMetrics, compactMetricLabel } from "../src/metrics.js";

const snapshot = {
  id: "codex",
  metrics: [
    { id: "five_hour", label: "5-hour window", percent: 34 },
    { id: "weekly", label: "Weekly limit", percent: 67 },
    { id: "monthly", label: "Monthly limit", percent: 12 },
  ],
};

test("Auto prefers session and weekly metrics together", () => {
  assert.deepEqual(chooseCompactMetrics(snapshot).map((metric) => metric.id), ["five_hour", "weekly"]);
});

test("explicit stable IDs retain user order and cap at two", () => {
  assert.deepEqual(
    chooseCompactMetrics(snapshot, ["monthly", "weekly", "five_hour"]).map((metric) => metric.id),
    ["monthly", "weekly"],
  );
});

test("stale and provider-mismatched IDs fall back to this provider's Auto metrics", () => {
  assert.deepEqual(
    chooseCompactMetrics(snapshot, ["claude_weekly", "removed_metric"]).map((metric) => metric.id),
    ["five_hour", "weekly"],
  );
});

test("single and empty providers remain unambiguous", () => {
  assert.equal(chooseCompactMetrics({ metrics: [{ id: "daily", label: "Daily", percent: 9 }] }).length, 1);
  assert.deepEqual(chooseCompactMetrics({ metrics: [] }), []);
});

test("compact labels are localized", () => {
  assert.equal(compactMetricLabel(snapshot.metrics[0], "ru"), "5\u0447");
  assert.equal(compactMetricLabel(snapshot.metrics[1], "zh"), "\u5468");
  assert.equal(compactMetricLabel(snapshot.metrics[2], "en"), "Mont");
});
