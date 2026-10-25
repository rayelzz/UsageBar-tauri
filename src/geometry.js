export function popupArrow(edge) {
  if (edge === "left") return "left";
  if (edge === "top") return "up";
  if (edge === "bottom") return "down";
  return "right";
}

export function edgePopupRect({ bar, edge, card, along, gap = 6 }) {
  const vertical = edge === "left" || edge === "right" || edge === "floating";
  const anchor = Number.isFinite(along)
    ? along
    : vertical
      ? bar.h / 2
      : bar.w / 2;

  if (edge === "left") {
    return { x: bar.x + bar.w + gap, y: bar.y + anchor - card.h / 2, w: card.w, h: card.h };
  }
  if (edge === "top") {
    return { x: bar.x + anchor - card.w / 2, y: bar.y + bar.h + gap, w: card.w, h: card.h };
  }
  if (edge === "bottom") {
    return { x: bar.x + anchor - card.w / 2, y: bar.y - card.h - gap, w: card.w, h: card.h };
  }
  return { x: bar.x - card.w - gap, y: bar.y + anchor - card.h / 2, w: card.w, h: card.h };
}

export function clampRectToWorkArea(rect, workArea, margin = 6) {
  if (!rect || !workArea) return rect;
  const minX = workArea.wx + margin;
  const minY = workArea.wy + margin;
  const maxX = Math.max(minX, workArea.wx + workArea.ww - rect.w - margin);
  const maxY = Math.max(minY, workArea.wy + workArea.wh - rect.h - margin);
  return {
    ...rect,
    x: Math.min(Math.max(rect.x, minX), maxX),
    y: Math.min(Math.max(rect.y, minY), maxY),
  };
}
