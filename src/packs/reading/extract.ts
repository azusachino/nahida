import { parseHTML } from "linkedom";
import { extractText, getDocumentProxy } from "unpdf";

export interface Passage {
  anchor: string;
  text: string;
  title?: string;
}
export const MAX_PDF_BYTES = 10 * 1024 * 1024;
const MAX_TEXT = 200_000;

export async function pdfPassages(bytes: Uint8Array): Promise<Passage[]> {
  if (bytes.length > MAX_PDF_BYTES) throw new Error("PDF exceeds 10 MiB");
  // unpdf's documented byte-input API; no URL, font, cmap or script evaluation.
  const pdf = await getDocumentProxy(new Uint8Array(bytes), {
    verbosity: 0,
    useSystemFonts: false,
    disableFontFace: true,
    useWorkerFetch: false,
    cMapUrl: undefined,
    standardFontDataUrl: undefined,
    wasmUrl: undefined,
    maxImageSize: 0,
    enableXfa: false,
  });
  try {
    if (pdf.numPages > 200) throw new Error("PDF exceeds 200 pages");
    const { text } = await extractText(pdf, { mergePages: false });
    if (text.join("").length > MAX_TEXT)
      throw new Error("extracted text exceeds 200000 characters");
    if (!text.some((page) => page.trim()))
      throw new Error("PDF has no extractable text; OCR is not supported");
    return text.map((page, i) => ({ anchor: `page:${i + 1}`, text: page }));
  } finally {
    await pdf.loadingTask.destroy();
  }
}

/** DOM parsing decodes entities without executing scripts or fetching subresources. */
export function htmlPassages(html: string): Passage[] {
  const { document } = parseHTML(html);
  for (const node of document.querySelectorAll(
    "script,style,noscript,template,nav,header,footer,aside,[hidden],[aria-hidden=true]",
  ))
    node.remove();
  const root =
    document.querySelector("article") ??
    document.querySelector("main") ??
    document.body;
  const passages: Passage[] = [];
  let current: Passage = { anchor: "", text: "" };
  const finish = () => {
    current.text = current.text
      .replace(/[ \t]+/g, " ")
      .replace(/ *\n */g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
    if (current.text)
      passages.push({ ...current, anchor: `section:${passages.length + 1}` });
  };
  type HtmlNode = (typeof root.childNodes)[number];
  const stack: (HtmlNode | string)[] = Array.from(root.childNodes).reverse();
  while (stack.length) {
    const node = stack.pop();
    if (typeof node === "string") {
      current.text += node;
      continue;
    }
    if (!node) continue;
    if (node.nodeType === 3) {
      current.text += node.textContent ?? "";
      continue;
    }
    if (node.nodeType !== 1) continue;
    const element = node as typeof root;
    if (/^H[1-6]$/.test(element.tagName)) {
      finish();
      const title = element.textContent?.replace(/\s+/g, " ").trim() ?? "";
      current = { anchor: "", title, text: `${title}\n\n` };
      continue;
    }
    if (/^(P|DIV|LI|PRE|BLOCKQUOTE|SECTION|BR|TR)$/.test(element.tagName)) {
      current.text += "\n";
      stack.push("\n");
    }
    stack.push(...Array.from(element.childNodes).reverse());
  }
  finish();
  if (!passages.length) throw new Error("article has no extractable text");
  if (passages.reduce((count, p) => count + p.text.length, 0) > MAX_TEXT)
    throw new Error("extracted text exceeds 200000 characters");
  return passages;
}
