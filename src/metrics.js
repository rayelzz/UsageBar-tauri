function metricKind(metric) {
  const text = `${metric?.id || ""} ${metric?.label || ""}`.toLowerCase();
  if (/weekly|week|7[_ -]?day/.test(text)) return "weekly";
  if (/5[_ -]?hour|five[_ -]?hour|session|primary/.test(text)) return "session";
  return "other";
}

export function chooseCompactMetrics(snapshot, configuredIds = []) {
  const metrics = Array.isArray(snapshot?.metrics) ? snapshot.metrics.filter((metric) => metric?.id) : [];
  const explicit = [...new Set(configuredIds || [])]
    .map((id) => metrics.find((metric) => metric.id === id))
    .filter(Boolean)
    .slice(0, 2);
  if (explicit.length) return explicit;

  const picked = [];
  const add = (metric) => {
    if (metric && !picked.some((item) => item.id === metric.id)) picked.push(metric);
  };
  add(metrics.find((metric) => metricKind(metric) === "session"));
  add(metrics.find((metric) => metricKind(metric) === "weekly"));
  for (const metric of metrics) {
    if (picked.length >= 2) break;
    add(metric);
  }
  return picked.slice(0, 2);
}

export function compactMetricLabel(metric, locale = "en") {
  const kind = metricKind(metric);
  if (kind === "session") return locale === "zh" ? "5\u65f6" : locale === "ru" ? "5\u0447" : "5h";
  if (kind === "weekly") return locale === "zh" ? "\u5468" : locale === "ru" ? "\u043d\u0435\u0434" : "wk";
  const raw = String(metric?.label || metric?.id || "").trim();
  const word = raw.split(/[\s\u00b7:_-]+/).find(Boolean) || "";
  return word.slice(0, 4);
}
