// Tiny markdown converter → DOM. Supported subset only (WIDGET_SPEC.md):
//   bold, italic, inline code, fenced code, bullet lists, links.
// Block rules:
//   ``` fenced code ```                → copy chip (mono)
//   a line that is only a link         → link row
//   - / * bullet lines                 → <ul>
//   everything else                    → paragraphs with inline formatting
// Inline links that are NOT alone on a line become inline anchors.
// No images, no tables, no raw HTML — anything else renders as literal text.

import { copyChip, linkRow, el, type RenderCtx } from "./dom";

const FENCE = /^\s*```/;
const BULLET = /^\s*[-*]\s+(.*)$/;
const LINK_ONLY = /^\s*\[([^\]]+)\]\(([^)\s]+)\)\s*$/;
const BARE_URL_ONLY = /^\s*(https?:\/\/\S+)\s*$/;

// Render a markdown string into a scoped `.md` container.
export function renderMarkdown(md: string, ctx: RenderCtx): HTMLElement {
  const root = el("div", "md");
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length === 0) return;
    const p = el("p", "md-p");
    appendInline(p, paragraph.join(" "), ctx);
    root.append(p);
    paragraph = [];
  };

  while (i < lines.length) {
    const line = lines[i] ?? "";

    // Fenced code → copy chip.
    if (FENCE.test(line)) {
      flushParagraph();
      const code: string[] = [];
      i++;
      while (i < lines.length && !FENCE.test(lines[i] ?? "")) {
        code.push(lines[i] ?? "");
        i++;
      }
      i++; // consume closing fence (if present)
      root.append(copyChip(code.join("\n"), undefined, true, ctx));
      continue;
    }

    // Blank line → paragraph break.
    if (line.trim() === "") {
      flushParagraph();
      i++;
      continue;
    }

    // A line that is only a link → link row.
    const linkOnly = LINK_ONLY.exec(line);
    if (linkOnly) {
      flushParagraph();
      root.append(linkRow(linkOnly[1], linkOnly[2] ?? "", ctx));
      i++;
      continue;
    }
    const bareOnly = BARE_URL_ONLY.exec(line);
    if (bareOnly) {
      flushParagraph();
      root.append(linkRow(undefined, bareOnly[1] ?? "", ctx));
      i++;
      continue;
    }

    // Bullet list.
    if (BULLET.test(line)) {
      flushParagraph();
      const list = el("ul", "md-list");
      while (i < lines.length) {
        const m = BULLET.exec(lines[i] ?? "");
        if (!m) break;
        const li = el("li");
        appendInline(li, m[1] ?? "", ctx);
        list.append(li);
        i++;
      }
      root.append(list);
      continue;
    }

    // Plain paragraph line.
    paragraph.push(line.trim());
    i++;
  }
  flushParagraph();
  return root;
}

// --- inline formatting ---------------------------------------------------
interface Matcher {
  re: RegExp;
  build: (m: RegExpExecArray, ctx: RenderCtx) => Node;
  nest: boolean; // parse the captured text recursively for nested formatting
}

// Ordered by precedence; ties on match index resolve to the earlier entry.
const MATCHERS: Matcher[] = [
  { re: /`([^`]+)`/, nest: false, build: (m) => el("code", "md-code", m[1]) },
  {
    re: /\[([^\]]+)\]\(([^)\s]+)\)/,
    nest: false,
    build: (m, ctx) => inlineAnchor(m[1] ?? "", m[2] ?? "", ctx),
  },
  { re: /\*\*([^*]+?)\*\*/, nest: true, build: (m, ctx) => wrap("strong", m[1] ?? "", ctx) },
  { re: /__([^_]+?)__/, nest: true, build: (m, ctx) => wrap("strong", m[1] ?? "", ctx) },
  { re: /\*([^*]+?)\*/, nest: true, build: (m, ctx) => wrap("em", m[1] ?? "", ctx) },
  { re: /_([^_]+?)_/, nest: true, build: (m, ctx) => wrap("em", m[1] ?? "", ctx) },
  {
    re: /(https?:\/\/[^\s<]+)/,
    nest: false,
    build: (m, ctx) => {
      const url = trimUrl(m[1] ?? "");
      return inlineAnchor(url, url, ctx);
    },
  },
];

function appendInline(target: Node, text: string, ctx: RenderCtx): void {
  for (const node of parseInline(text, ctx)) target.appendChild(node);
}

function parseInline(text: string, ctx: RenderCtx): Node[] {
  const out: Node[] = [];
  let rest = text;
  while (rest.length > 0) {
    let bestIdx = -1;
    let best: { matcher: Matcher; m: RegExpExecArray } | null = null;
    for (const matcher of MATCHERS) {
      const m = matcher.re.exec(rest);
      if (m && (bestIdx === -1 || m.index < bestIdx)) {
        bestIdx = m.index;
        best = { matcher, m };
      }
    }
    if (!best) {
      out.push(document.createTextNode(rest));
      break;
    }
    if (best.m.index > 0) {
      out.push(document.createTextNode(rest.slice(0, best.m.index)));
    }
    out.push(best.matcher.build(best.m, ctx));
    rest = rest.slice(best.m.index + best.m[0].length);
  }
  return out;
}

function wrap(tag: "strong" | "em", inner: string, ctx: RenderCtx): Node {
  const node = el(tag);
  appendInline(node, inner, ctx);
  return node;
}

function inlineAnchor(label: string, url: string, ctx: RenderCtx): Node {
  const a = el("span", "md-link", label);
  a.setAttribute("role", "link");
  a.tabIndex = 0;
  a.addEventListener("click", () => ctx.openLink(url));
  return a;
}

function trimUrl(url: string): string {
  return url.replace(/[.,;:)\]]+$/, "");
}
