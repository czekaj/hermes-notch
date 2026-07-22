// Collapsed strip — the black notch extension. Answers "what next" in one
// saccade: widget icon + one line + dimmed detail, with a 3px urgency dot.
// The strip is pure #000 in both themes; its text is always light.

import type { Card, ConnState, Urgency, WidgetSpec } from "../types";
import { el } from "../render/dom";

const STALE_GLYPH = "◷"; // monochrome clock, not an emoji

export interface StripData {
  conn: ConnState;
  hostLabel: string;
  widgets: WidgetSpec[];
  glances: Record<string, Card>;
  shownId: string | null;
}

export function renderStrip(data: StripData): HTMLElement {
  const strip = el("div", "strip");
  const inner = el("div", "strip-inner");
  strip.append(inner);

  // Connection-driven states take precedence over any glance.
  if (data.conn === "connecting" || data.conn === "disconnected") {
    inner.classList.add("is-status");
    inner.append(el("span", "strip-status", "— connecting to hermes —"));
    return strip;
  }

  if (data.conn === "error") {
    inner.classList.add("is-status");
    inner.append(el("span", "strip-status", data.hostLabel || "hermes host"));
    inner.append(el("span", "strip-errdot"));
    return strip;
  }

  if (data.widgets.length === 0) {
    inner.classList.add("is-status");
    inner.append(el("span", "strip-status", "no widgets — install some"));
    return strip;
  }

  const spec = data.widgets.find((w) => w.id === data.shownId) ?? data.widgets[0];
  const card = spec ? data.glances[spec.id] : undefined;
  const glance = card?.glance;

  if (spec?.icon) inner.append(el("span", "strip-icon", spec.icon));

  const body = el("div", "strip-body");
  const line = el("div", "strip-line");

  const text = el("span", "strip-text", glance?.text ?? spec?.name ?? "Hermes");
  line.append(text);

  const stale = card?.status?.state === "stale";
  const detailText = glance?.detail ?? "";
  if (stale || detailText) {
    const detail = el("span", "strip-detail");
    if (stale) detail.append(el("span", "strip-clock", STALE_GLYPH));
    if (detailText) detail.append(document.createTextNode(detailText));
    line.append(detail);
  }
  body.append(line);

  const urgency: Urgency = glance?.urgency ?? "normal";
  const underline = el("div", "strip-underline");
  underline.append(el("span", `strip-dot u-${urgency}`));
  body.append(underline);

  inner.append(body);
  return strip;
}
