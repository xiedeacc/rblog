import mermaid from "mermaid";

const MERMAID_SELECTOR = "pre.mermaid, pre[lang='mermaid'], pre > code.language-mermaid";
let initialized = false;

function initializeMermaid() {
  if (initialized) return;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    // HTML labels are rendered as SVG foreignObject nodes. Browsers mark a
    // canvas that draws such an SVG as origin-tainted, so it cannot be copied
    // or exported as PNG. Native SVG labels keep diagrams exportable.
    htmlLabels: false,
  });
  initialized = true;
}

/**
 * Convert both current rblog Mermaid blocks and legacy highlighted code
 * blocks into diagrams. Rendering one node at a time keeps a malformed
 * diagram from preventing valid diagrams later in the article from loading.
 */
export async function renderMermaid(root: ParentNode): Promise<void> {
  const candidates = [...root.querySelectorAll<HTMLElement>(MERMAID_SELECTOR)];
  if (!candidates.length) return;

  initializeMermaid();

  for (const candidate of candidates) {
    const node = candidate.matches("code.language-mermaid")
      ? candidate.parentElement
      : candidate;
    if (!node || node.dataset.processed) continue;

    const source = candidate.textContent ?? "";
    node.classList.add("mermaid");
    node.removeAttribute("lang");
    node.textContent = source;

    try {
      await mermaid.run({ nodes: [node], suppressErrors: true });
    } catch (error) {
      node.removeAttribute("data-processed");
      node.classList.add("mermaid-error");
      node.textContent = source;
      node.title = error instanceof Error ? error.message : "Mermaid rendering failed";
    }
  }
}
