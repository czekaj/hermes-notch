// Shared low-level DOM builders used by both the Card renderer and the markdown
// converter. Kept separate so card.ts ⇄ markdown.ts don't form an import cycle.

export interface RenderCtx {
  // Copy chip clicked → write to the viewing Mac's clipboard.
  copy: (text: string) => void;
  // Link clicked → open on the viewing Mac and collapse the panel.
  openLink: (url: string) => void;
}

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

const COPY_GLYPH = "⧉"; // ⧉
const CHECK_GLYPH = "✓"; // ✓

// A one-tap copy chip. `mono` renders the value in the monospace face.
export function copyChip(
  value: string,
  label: string | undefined,
  mono: boolean,
  ctx: RenderCtx,
): HTMLElement {
  const chip = el("button", "copy-chip");
  chip.type = "button";

  const ico = el("span", "chip-ico", COPY_GLYPH);
  chip.append(ico);

  const bodyWrap = el("span", "chip-body");
  if (label) bodyWrap.append(el("span", "chip-label", label));
  bodyWrap.append(el("span", `chip-value${mono ? " mono" : ""}`, value));
  chip.append(bodyWrap);

  let flashTimer = 0;
  chip.addEventListener("click", () => {
    ctx.copy(value);
    chip.classList.add("copied");
    ico.textContent = CHECK_GLYPH;
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      chip.classList.remove("copied");
      ico.textContent = COPY_GLYPH;
    }, 900);
  });
  return chip;
}

const LINK_GLYPH = "↗"; // ↗

// A link row: opens on single click and collapses the panel.
export function linkRow(
  label: string | undefined,
  url: string,
  ctx: RenderCtx,
): HTMLElement {
  const row = el("button", "link-row");
  row.type = "button";
  row.append(el("span", "link-ico", LINK_GLYPH));
  row.append(el("span", "link-label", label && label.trim() ? label : url));
  row.addEventListener("click", () => ctx.openLink(url));
  return row;
}
