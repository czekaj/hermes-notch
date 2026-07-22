// Card body → DOM. One function per block type (WIDGET_SPEC.md).
// Unknown block types are skipped so the schema can grow without breaking us.

import type { Block, Card } from "../types";
import { copyChip, linkRow, el, type RenderCtx } from "./dom";
import { renderMarkdown } from "./markdown";

export type { RenderCtx };

// Render the ordered blocks of a Card into a fresh `.body-blocks` container.
export function renderBlocks(card: Card, ctx: RenderCtx): HTMLElement {
  const wrap = el("div", "body-blocks");
  const blocks = card.body ?? [];
  for (const block of blocks) {
    const node = renderBlock(block, ctx);
    if (node) wrap.append(node);
  }
  return wrap;
}

export function renderBlock(block: Block, ctx: RenderCtx): HTMLElement | null {
  switch (block.type) {
    case "text":
      return el("div", "b-text", block.text);

    case "md":
      return renderMarkdown(block.text, ctx);

    case "copy":
      return copyChip(block.value, block.label, block.mono === true, ctx);

    case "link":
      return linkRow(block.label, block.url, ctx);

    case "kv":
      return renderKv(block.items);

    case "progress":
      return renderProgress(block.value, block.max, block.label);

    case "divider":
      return el("div", "divider");

    default:
      // Unknown block type — skip silently (forward compatibility).
      return null;
  }
}

function renderKv(items: Array<[string, string]>): HTMLElement {
  const kv = el("div", "kv");
  for (const pair of items) {
    const row = el("div", "kv-row");
    row.append(el("span", "kv-k", pair[0] ?? ""));
    row.append(el("span", "kv-v", pair[1] ?? ""));
    kv.append(row);
  }
  return kv;
}

function renderProgress(
  value: number,
  max: number,
  label: string | undefined,
): HTMLElement {
  const wrap = el("div", "progress");
  const safeMax = max > 0 ? max : 1;
  const pct = Math.max(0, Math.min(1, value / safeMax)) * 100;

  const track = el("div", "progress-track");
  const fill = el("div", "progress-fill");
  fill.style.width = `${pct}%`;
  track.append(fill);
  wrap.append(track);

  const caption = el("div", "progress-label");
  const parts: string[] = [];
  if (label) parts.push(label);
  parts.push(`${value}/${max}`);
  caption.textContent = parts.join(" ");
  wrap.append(caption);

  return wrap;
}
