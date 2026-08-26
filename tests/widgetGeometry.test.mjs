import assert from "node:assert/strict";
import test from "node:test";
import { calculateEdgeDock, clampWidgetPosition } from "../src/widgetGeometry.ts";

const area = { position: { x: 0, y: 20 }, size: { width: 1440, height: 880 } };
const size = { width: 210, height: 42 };

test("keeps a widget away from screen edges freely movable", () => {
  assert.equal(calculateEdgeDock({ x: 400, y: 300 }, size, area, 1), null);
});

test("hides against all four edges while leaving ten points visible", () => {
  assert.deepEqual(calculateEdgeDock({ x: 5, y: 300 }, size, area, 1), {
    side: "left", exposed: { x: 0, y: 300 }, hidden: { x: -200, y: 300 },
  });
  assert.deepEqual(calculateEdgeDock({ x: 1228, y: 300 }, size, area, 1), {
    side: "right", exposed: { x: 1230, y: 300 }, hidden: { x: 1430, y: 300 },
  });
  assert.deepEqual(calculateEdgeDock({ x: 400, y: 22 }, size, area, 1), {
    side: "top", exposed: { x: 400, y: 20 }, hidden: { x: 400, y: -12 },
  });
  assert.deepEqual(calculateEdgeDock({ x: 400, y: 856 }, size, area, 1), {
    side: "bottom", exposed: { x: 400, y: 858 }, hidden: { x: 400, y: 890 },
  });
});

test("uses physical pixels at Retina scale and clamps the parallel axis", () => {
  const retinaArea = { position: { x: 1440, y: 0 }, size: { width: 2560, height: 1800 } };
  assert.deepEqual(
    calculateEdgeDock({ x: 1450, y: -40 }, { width: 420, height: 84 }, retinaArea, 2),
    { side: "left", exposed: { x: 1440, y: 0 }, hidden: { x: 1040, y: 0 } },
  );
});

test("recovers a persisted widget position after a monitor is removed", () => {
  assert.deepEqual(clampWidgetPosition({ x: 2500, y: -300 }, size, area), {
    x: 1230,
    y: 20,
  });
});
