import { renderMermaid } from "@/lib/mermaid";
import { installMermaidImagePopup } from "@/lib/mermaid-image-popup";

void renderMermaid(document).then(() => installMermaidImagePopup(document));
