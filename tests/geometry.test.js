import assert from "node:assert/strict";
import test from "node:test";

import { clampRectToWorkArea, edgePopupRect, popupArrow } from "../src/geometry.js";

const bar = { x: 100, y: 200, w: 60, h: 300 };
const card = { w: 240, h: 120 };

for (const row of [
  ["right", { x: -146, y: 290 }, "right"],
  ["floating", { x: -146, y: 290 }, "right"],
  ["left", { x: 166, y: 290 }, "left"],
  ["top", { x: 130, y: 506 }, "up"],
  ["bottom", { x: 130, y: 74 }, "down"],
]) {
  test(`places ${row[0]} popover on the inward side`, () => {
    const actual = edgePopupRect({ bar, edge: row[0], card, along: 150 });
    assert.deepEqual(actual, { ...row[1], ...card });
    assert.equal(popupArrow(row[0]), row[2]);
  });
}

test("uses the supplied card size and along-edge anchor", () => {
  assert.deepEqual(
    edgePopupRect({ bar, edge: "bottom", card: { w: 360, h: 220 }, along: 20 }),
    { x: -60, y: -26, w: 360, h: 220 },
  );
});

test("clamps within a negative-coordinate monitor work area", () => {
  const screen = { wx: -1920, wy: -40, ww: 1920, wh: 1040 };
  assert.deepEqual(
    clampRectToWorkArea({ x: -2100, y: 930, w: 320, h: 180 }, screen),
    { x: -1914, y: 814, w: 320, h: 180 },
  );
});

test("keeps oversized popovers anchored at the work-area margin", () => {
  const screen = { wx: 1920, wy: 0, ww: 240, wh: 180 };
  assert.deepEqual(
    clampRectToWorkArea({ x: 2200, y: 200, w: 400, h: 300 }, screen),
    { x: 1926, y: 6, w: 400, h: 300 },
  );
});
