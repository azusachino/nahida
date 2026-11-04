import { createHash } from "node:crypto";
import { fstatSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { relative } from "node:path";
import type { AgentTool, AgentToolResult } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import type { Pack } from "../../pack";
import { workspace } from "../../workspace";
import {
  htmlPassages,
  MAX_PDF_BYTES,
  type Passage,
  pdfPassages,
} from "./extract";
import prompt from "./prompt.md" with { type: "text" };
import { articleUrl, readHtml } from "./web";

const pdfParameters = Type.Object({ path: Type.String({ minLength: 1 }) });
const articleParameters = Type.Object({ url: Type.String({ minLength: 1 }) });
const noteParameters = Type.Object({
  path: Type.String({ minLength: 1 }),
  source: Type.String({ minLength: 1 }),
  anchor: Type.String({ minLength: 1 }),
  highlight: Type.Optional(Type.String({ minLength: 1, maxLength: 20000 })),
  note: Type.Optional(Type.String({ minLength: 1, maxLength: 20000 })),
});
interface Extracted {
  source: string;
  sha256: string;
  passages: Passage[];
}

async function result(
  action: () => unknown | Promise<unknown>,
): Promise<AgentToolResult> {
  try {
    const value = await action();
    return {
      content: [
        {
          type: "text",
          text: typeof value === "string" ? value : JSON.stringify(value),
        },
      ],
      details: undefined,
    };
  } catch (error) {
    return {
      content: [
        {
          type: "text",
          text: error instanceof Error ? error.message : String(error),
        },
      ],
      details: undefined,
      isError: true,
    };
  }
}

export function readingPack(
  cwd: string,
  dependencies: { readHtml?: typeof readHtml } = {},
): Pack {
  const root = realpathSync(cwd);
  const files = workspace(root);
  const sources = new Map<string, Extracted>();
  const remember = (
    source: string,
    bytes: Uint8Array | string,
    passages: Passage[],
  ) => {
    const extracted = {
      source,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      passages,
    };
    sources.set(source, extracted);
    return extracted;
  };
  const pdfTool: AgentTool<typeof pdfParameters> = {
    name: "read_pdf",
    label: "Read PDF",
    description:
      "Extract a local workspace PDF into page-anchored text. EPUB and OCR are not supported.",
    parameters: pdfParameters,
    executionMode: "sequential",
    execute: async (_id, { path }, signal) =>
      result(async () => {
        signal?.throwIfAborted();
        const target = files.path(path, [".pdf"]);
        const bytes = files.withFile(target, false, (fd) => {
          if (fstatSync(fd).size > MAX_PDF_BYTES)
            throw new Error("PDF exceeds 10 MiB");
          return readFileSync(fd);
        });
        const passages = await pdfPassages(bytes);
        signal?.throwIfAborted();
        return remember(relative(root, target), bytes, passages);
      }),
  };
  const articleTool: AgentTool<typeof articleParameters> = {
    name: "read_article",
    label: "Read article",
    description:
      "Extract a public HTTPS HTML article into numbered section anchors. No redirects, credentials or private hosts.",
    parameters: articleParameters,
    executionMode: "sequential",
    execute: async (_id, { url }, signal) =>
      result(async () => {
        signal?.throwIfAborted();
        const source = articleUrl(url).href;
        const html = await (dependencies.readHtml ?? readHtml)(source, {
          signal,
        });
        signal?.throwIfAborted();
        return remember(source, html, htmlPassages(html));
      }),
  };
  const noteTool: AgentTool<typeof noteParameters> = {
    name: "record_note",
    label: "Record reading note",
    description:
      "Append an anchored highlight and/or note to a workspace Markdown file. Source and anchor must have been extracted in this run; highlights must match the passage exactly.",
    parameters: noteParameters,
    executionMode: "sequential",
    execute: async (_id, { path, source, anchor, highlight, note }, signal) =>
      result(() => {
        signal?.throwIfAborted();
        const extracted = sources.get(source);
        const passage = extracted?.passages.find(
          (item) => item.anchor === anchor,
        );
        if (!extracted || !passage)
          throw new Error(
            "extract the source and anchor in this run before recording a note",
          );
        if (!highlight?.trim() && !note?.trim())
          throw new Error("provide a highlight or note");
        if (highlight && !passage.text.includes(highlight))
          throw new Error("highlight must match the anchored passage exactly");
        const block = `\n## Reading note\n\nSource: ${JSON.stringify(source)}\n\nRevision: sha256:${extracted.sha256}\n\nAnchor: ${anchor}\n\n${
          highlight
            ? `${highlight
                .split("\n")
                .map((line) => `> ${line}`)
                .join("\n")}\n\n`
            : ""
        }${note ? `${note}\n` : ""}`;
        const target = files.path(path, [".md", ".markdown"], true);
        try {
          writeFileSync(target, block, {
            encoding: "utf8",
            flag: "wx",
            mode: 0o600,
          });
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
          files.withFile(target, true, (fd) => {
            // Reading advances the descriptor to EOF; validate encoding before appending.
            new TextDecoder("utf-8", { fatal: true }).decode(readFileSync(fd));
            writeFileSync(fd, block, "utf8");
          });
        }
        return `Recorded ${source} ${anchor} in ${path}`;
      }),
  };
  return {
    id: "reading",
    prompt: { file: "prompt.md", text: prompt },
    tools: [pdfTool, articleTool, noteTool],
  };
}
