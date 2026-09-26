const MAX_CANVAS_DIMENSION = 8192;
const MAX_CANVAS_PIXELS = 40_000_000;

interface PopupElements {
  overlay: HTMLDivElement;
  image: HTMLImageElement;
  status: HTMLParagraphElement;
  copy: HTMLButtonElement;
  download: HTMLAnchorElement;
}

interface GeneratedPng {
  blob: Blob;
  width: number;
  height: number;
}

let installed = false;
let popup: PopupElements | null = null;
let pngBlob: Blob | null = null;
let imageUrl: string | null = null;
let generation = 0;

function makeElement<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  if (text) element.textContent = text;
  return element;
}

function closePopup() {
  if (!popup) return;
  generation += 1;
  popup.overlay.hidden = true;
  document.body.classList.remove("mermaid-popup-open");
  if (imageUrl) URL.revokeObjectURL(imageUrl);
  imageUrl = null;
  pngBlob = null;
  popup.image.removeAttribute("src");
}

function getPopup(): PopupElements {
  if (popup) return popup;

  const overlay = makeElement("div", "mermaid-popup");
  overlay.hidden = true;
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.setAttribute("aria-label", "Mermaid 高清图片预览");

  const panel = makeElement("div", "mermaid-popup__panel");
  const header = makeElement("div", "mermaid-popup__header");
  const title = makeElement("strong", "mermaid-popup__title", "Mermaid 高清图片");
  const close = makeElement("button", "mermaid-popup__close", "×");
  close.type = "button";
  close.setAttribute("aria-label", "关闭");
  close.addEventListener("click", closePopup);
  header.append(title, close);

  const viewport = makeElement("div", "mermaid-popup__viewport");
  const image = makeElement("img", "mermaid-popup__image");
  image.alt = "自动生成的 Mermaid 高清图片";
  image.hidden = true;
  viewport.append(image);

  const footer = makeElement("div", "mermaid-popup__footer");
  const status = makeElement("p", "mermaid-popup__status", "正在生成高清图片…");
  const actions = makeElement("div", "mermaid-popup__actions");
  const download = makeElement("a", "mermaid-popup__download", "下载 PNG");
  download.setAttribute("role", "button");
  download.hidden = true;
  const copy = makeElement("button", "mermaid-popup__copy", "复制图片");
  copy.type = "button";
  copy.disabled = true;
  copy.addEventListener("click", async () => {
    if (!pngBlob) return;
    try {
      if (!navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
        throw new Error("当前浏览器不支持复制图片，请使用下载 PNG");
      }
      await navigator.clipboard.write([
        new ClipboardItem({ "image/png": pngBlob }),
      ]);
      status.textContent = "图片已复制到剪贴板";
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "复制图片失败";
    }
  });
  actions.append(download, copy);
  footer.append(status, actions);
  panel.append(header, viewport, footer);
  overlay.append(panel);

  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) closePopup();
  });
  document.body.append(overlay);
  popup = { overlay, image, status, copy, download };
  return popup;
}

function svgDimensions(svg: SVGSVGElement): { width: number; height: number } {
  const viewBox = svg.viewBox.baseVal;
  const bounds = svg.getBoundingClientRect();
  const width = viewBox.width || svg.width.baseVal.value || bounds.width;
  const height = viewBox.height || svg.height.baseVal.value || bounds.height;
  if (width <= 0 || height <= 0) {
    throw new Error("无法读取图表尺寸");
  }
  return { width, height };
}

function foreignObjectText(node: SVGForeignObjectElement): string[] {
  const content = node.cloneNode(true) as SVGForeignObjectElement;
  content.querySelectorAll("br").forEach((breakElement) => {
    breakElement.replaceWith(document.createTextNode("\n"));
  });
  return (content.textContent ?? "")
    .split("\n")
    .map((line) => line.replace(/\s+/g, " ").trim())
    .filter(Boolean);
}

/**
 * Mermaid directives in older posts can opt back into HTML labels even when
 * the global renderer uses native SVG labels. Replace those foreignObject
 * nodes before rasterization so the canvas remains exportable.
 */
function replaceForeignObjects(svg: SVGSVGElement) {
  const namespace = "http://www.w3.org/2000/svg";
  svg.querySelectorAll<SVGForeignObjectElement>("foreignObject").forEach((node) => {
    const lines = foreignObjectText(node);
    const x = Number.parseFloat(node.getAttribute("x") ?? "0") || 0;
    const y = Number.parseFloat(node.getAttribute("y") ?? "0") || 0;
    const width = Number.parseFloat(node.getAttribute("width") ?? "0") || 0;
    const height = Number.parseFloat(node.getAttribute("height") ?? "0") || 0;
    const text = document.createElementNS(namespace, "text");
    text.setAttribute("x", String(x + width / 2));
    text.setAttribute("y", String(y + height / 2));
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("dominant-baseline", "middle");
    text.setAttribute("font-family", "Arial, sans-serif");
    text.setAttribute("font-size", "14");
    text.setAttribute("fill", "#333");

    if (lines.length <= 1) {
      text.textContent = lines[0] ?? "";
    } else {
      lines.forEach((line, index) => {
        const span = document.createElementNS(namespace, "tspan");
        span.setAttribute("x", String(x + width / 2));
        span.setAttribute("dy", index === 0 ? `${-(lines.length - 1) * 0.6}em` : "1.2em");
        span.textContent = line;
        text.append(span);
      });
    }
    node.replaceWith(text);
  });
}

async function svgToPng(svg: SVGSVGElement): Promise<GeneratedPng> {
  const { width, height } = svgDimensions(svg);
  const requestedScale = Math.max(3, window.devicePixelRatio || 1);
  const dimensionScale = MAX_CANVAS_DIMENSION / Math.max(width, height);
  const pixelScale = Math.sqrt(MAX_CANVAS_PIXELS / (width * height));
  const scale = Math.max(1, Math.min(requestedScale, dimensionScale, pixelScale));

  const clone = svg.cloneNode(true) as SVGSVGElement;
  clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  clone.setAttribute("width", String(width));
  clone.setAttribute("height", String(height));
  replaceForeignObjects(clone);

  const source = new XMLSerializer().serializeToString(clone);
  const sourceUrl = URL.createObjectURL(
    new Blob([source], { type: "image/svg+xml;charset=utf-8" }),
  );

  try {
    const rendered = new Image();
    rendered.decoding = "async";
    await new Promise<void>((resolve, reject) => {
      rendered.onload = () => resolve();
      rendered.onerror = () => reject(new Error("无法生成 Mermaid 图片"));
      rendered.src = sourceUrl;
    });

    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(width * scale);
    canvas.height = Math.ceil(height * scale);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("浏览器不支持图片生成");
    context.fillStyle = "#fff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.drawImage(rendered, 0, 0, canvas.width, canvas.height);

    const blob = await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(
        (blob) => blob ? resolve(blob) : reject(new Error("PNG 图片生成失败")),
        "image/png",
      );
    });
    return { blob, width: canvas.width, height: canvas.height };
  } finally {
    URL.revokeObjectURL(sourceUrl);
  }
}

async function openPopup(svg: SVGSVGElement) {
  const elements = getPopup();
  const currentGeneration = ++generation;
  if (imageUrl) URL.revokeObjectURL(imageUrl);
  imageUrl = null;
  pngBlob = null;
  elements.image.hidden = true;
  elements.image.removeAttribute("src");
  elements.copy.disabled = true;
  elements.download.hidden = true;
  elements.status.textContent = "正在生成高清图片…";
  elements.overlay.hidden = false;
  document.body.classList.add("mermaid-popup-open");

  try {
    const generated = await svgToPng(svg);
    if (currentGeneration !== generation) return;
    pngBlob = generated.blob;
    imageUrl = URL.createObjectURL(generated.blob);
    elements.image.src = imageUrl;
    elements.image.hidden = false;
    elements.download.href = imageUrl;
    elements.download.download = `mermaid-${Date.now()}.png`;
    elements.download.hidden = false;
    elements.copy.disabled = false;
    elements.status.textContent = `${generated.width} × ${generated.height} PNG`;
  } catch (error) {
    if (currentGeneration !== generation) return;
    elements.status.textContent = error instanceof Error ? error.message : "图片生成失败";
  }
}

function diagramFromTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  const diagram = target.closest<HTMLElement>("pre.mermaid");
  if (!diagram || diagram.classList.contains("mermaid-error")) return null;
  return diagram.querySelector("svg") ? diagram : null;
}

export function installMermaidImagePopup(root: Document = document) {
  if (installed) return;
  installed = true;

  root.querySelectorAll<HTMLElement>("pre.mermaid").forEach((diagram) => {
    if (!diagram.querySelector("svg")) return;
    diagram.classList.add("mermaid-clickable");
    diagram.tabIndex = 0;
    diagram.setAttribute("role", "button");
    diagram.setAttribute("aria-label", "打开 Mermaid 高清图片");
    diagram.title = "点击查看并复制高清图片";
  });

  root.addEventListener("click", (event) => {
    const diagram = diagramFromTarget(event.target);
    const svg = diagram?.querySelector<SVGSVGElement>("svg");
    if (svg) void openPopup(svg);
  });
  root.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && popup && !popup.overlay.hidden) {
      closePopup();
      return;
    }
    if (event.key !== "Enter" && event.key !== " ") return;
    const diagram = diagramFromTarget(event.target);
    const svg = diagram?.querySelector<SVGSVGElement>("svg");
    if (!svg) return;
    event.preventDefault();
    void openPopup(svg);
  });
}
