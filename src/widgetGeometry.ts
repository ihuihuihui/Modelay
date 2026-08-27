export type WidgetPoint = { x: number; y: number };
export type WidgetSize = { width: number; height: number };
export type WidgetArea = { position: WidgetPoint; size: WidgetSize };
export type WidgetSide = "left" | "right" | "top" | "bottom";

export type EdgeDock = {
  side: WidgetSide;
  exposed: WidgetPoint;
  hidden: WidgetPoint;
};

export type WidgetEasing = "reveal" | "hide";

export function interpolateWidgetPosition(
  from: WidgetPoint,
  to: WidgetPoint,
  progress: number,
  easing: WidgetEasing,
): WidgetPoint {
  const bounded = clamp(progress, 0, 1);
  const eased = easing === "reveal"
    ? 1 - Math.pow(1 - bounded, 3)
    : Math.pow(bounded, 3);
  return {
    x: Math.round(from.x + (to.x - from.x) * eased),
    y: Math.round(from.y + (to.y - from.y) * eased),
  };
}

export function clampWidgetPosition(
  position: WidgetPoint,
  size: WidgetSize,
  area: WidgetArea,
): WidgetPoint {
  return {
    x: clamp(position.x, area.position.x, area.position.x + area.size.width - size.width),
    y: clamp(position.y, area.position.y, area.position.y + area.size.height - size.height),
  };
}

export function calculateEdgeDock(
  position: WidgetPoint,
  size: WidgetSize,
  area: WidgetArea,
  scaleFactor: number,
  edgePoints = 48,
  visiblePoints = 10,
): EdgeDock | null {
  const threshold = edgePoints * scaleFactor;
  const visible = visiblePoints * scaleFactor;
  const distances: Array<[WidgetSide, number]> = [
    ["left", Math.abs(position.x - area.position.x)],
    ["right", Math.abs(area.position.x + area.size.width - (position.x + size.width))],
    ["top", Math.abs(position.y - area.position.y)],
    ["bottom", Math.abs(area.position.y + area.size.height - (position.y + size.height))],
  ];
  distances.sort((left, right) => left[1] - right[1]);
  const [side, distance] = distances[0];
  if (distance > threshold) return null;

  const exposed = {
    x: side === "left"
      ? area.position.x
      : side === "right"
        ? area.position.x + area.size.width - size.width
        : clampWidgetPosition(position, size, area).x,
    y: side === "top"
      ? area.position.y
      : side === "bottom"
        ? area.position.y + area.size.height - size.height
        : clampWidgetPosition(position, size, area).y,
  };
  const hidden = {
    x: side === "left"
      ? area.position.x - size.width + visible
      : side === "right"
        ? area.position.x + area.size.width - visible
        : exposed.x,
    y: side === "top"
      ? area.position.y - size.height + visible
      : side === "bottom"
        ? area.position.y + area.size.height - visible
        : exposed.y,
  };
  return { side, exposed, hidden };
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(value, maximum));
}
