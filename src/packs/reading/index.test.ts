import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  truncateSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import type { AgentToolResult } from "@earendil-works/pi-agent-core";
import { htmlPassages, MAX_PDF_BYTES, pdfPassages } from "./extract";
import { readingPack } from "./index";

const fixtures = join(import.meta.dir, "fixtures");
const scratch = resolve(import.meta.dir, "../../../.tmp/reading-tests");
const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0))
    rmSync(root, { recursive: true, force: true });
});
const html = readFileSync(join(fixtures, "article.html"), "utf8");
function content(result: AgentToolResult): string {
  return result.content
    .map((block) => (block.type === "text" ? block.text : ""))
    .join("");
}
function fixture() {
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, "case-"));
  roots.push(root);
  const cwd = join(root, "workspace");
  mkdirSync(cwd);
  copyFileSync(join(fixtures, "two-pages.pdf"), join(cwd, "paper.pdf"));
  let requests = 0;
  const makePack = () =>
    readingPack(cwd, {
      readHtml: async () => {
        requests++;
        return html;
      },
    });
  const pack = makePack();
  const call = async (
    name: string,
    args: Record<string, unknown>,
    signal?: AbortSignal,
  ) => {
    const tool = pack.tools?.find((tool) => tool.name === name);
    if (!tool) throw new Error(`missing tool ${name}`);
    return tool.execute("fixture", args, signal);
  };
  return { root, cwd, call, makePack, requests: () => requests };
}

test("the committed PDF extracts into exact numbered page anchors", async () => {
  expect(
    await pdfPassages(readFileSync(join(fixtures, "two-pages.pdf"))),
  ).toEqual([
    { anchor: "page:1", text: "First page: anchored evidence." },
    { anchor: "page:2", text: "Second page: another passage." },
  ]);
});

test("HTML sections decode entities and exclude scripts, chrome and hidden material", () => {
  expect(htmlPassages(html)).toEqual([
    {
      anchor: "section:1",
      title: "A small & useful article",
      text: "A small & useful article\n\nFirst section: anchored evidence & a second sentence.",
    },
    {
      anchor: "section:2",
      title: "Observations",
      text: "Observations\n\nSecond section includes “quoted text” and 日本語.",
    },
  ]);
  expect(() =>
    htmlPassages("<html><body><script>secret</script></body></html>"),
  ).toThrow("no extractable text");
  expect(() =>
    htmlPassages(`<html><body>${"x".repeat(200001)}</body></html>`),
  ).toThrow("200000");
});

test("PDF extraction and note recording preserve source revision, page and exact highlight", async () => {
  const { cwd, call } = fixture();
  const read = await call("read_pdf", { path: "./paper.pdf" });
  expect(read.isError).not.toBe(true);
  const extracted = JSON.parse(content(read));
  const digest = createHash("sha256")
    .update(readFileSync(join(cwd, "paper.pdf")))
    .digest("hex");
  expect(extracted.source).toBe("paper.pdf");
  expect(extracted.sha256).toBe(digest);
  expect(
    (
      await call("record_note", {
        path: "notes/paper.md",
        source: extracted.source,
        anchor: "page:2",
        highlight: "another passage",
        note: "Compare with page 1.",
      })
    ).isError,
  ).not.toBe(true);
  expect(readFileSync(join(cwd, "notes/paper.md"), "utf8")).toBe(
    `\n## Reading note\n\nSource: "paper.pdf"\n\nRevision: sha256:${digest}\n\nAnchor: page:2\n\n> another passage\n\nCompare with page 1.\n`,
  );
});

test("article notes append to existing Markdown without losing prior content", async () => {
  const { cwd, call, requests } = fixture();
  const read = await call("read_article", {
    url: "https://article.example/story#old",
  });
  expect(requests()).toBe(1);
  const extracted = JSON.parse(content(read));
  expect(extracted.source).toBe("https://article.example/story");
  const path = join(cwd, "notes.md");
  writeFileSync(path, "\uFEFF# Existing\r\n");
  expect(
    (
      await call("record_note", {
        path: "notes.md",
        source: extracted.source,
        anchor: "section:2",
        highlight: "“quoted text”",
        note: "My observation.",
      })
    ).isError,
  ).not.toBe(true);
  const first = readFileSync(path, "utf8");
  expect(first).toStartWith("\uFEFF# Existing\r\n\n## Reading note");
  expect(first).toContain(
    "Anchor: section:2\n\n> “quoted text”\n\nMy observation.",
  );
  expect(
    (
      await call("record_note", {
        path: "notes.md",
        source: extracted.source,
        anchor: "section:1",
        note: "One more note.",
      })
    ).isError,
  ).not.toBe(true);
  expect(readFileSync(path, "utf8")).toStartWith(first);
  expect(readFileSync(path, "utf8")).toEndWith("One more note.\n");
});

test("unknown sources, invented anchors and unmatched highlights never create a note", async () => {
  const { cwd, call } = fixture();
  await call("read_pdf", { path: "paper.pdf" });
  for (const args of [
    { source: "unknown.pdf", anchor: "page:1", note: "note" },
    { source: "paper.pdf", anchor: "page:9", note: "note" },
    { source: "paper.pdf", anchor: "page:1", highlight: "invented quote" },
    { source: "paper.pdf", anchor: "page:1" },
  ]) {
    expect(
      (await call("record_note", { path: "new/notes.md", ...args })).isError,
    ).toBe(true);
  }
  expect(existsSync(join(cwd, "new"))).toBe(false);
});

test("invalid UTF-8 note files are refused without appending bytes", async () => {
  const { cwd, call } = fixture();
  await call("read_pdf", { path: "paper.pdf" });
  const path = join(cwd, "notes.md");
  const bytes = Buffer.from([0x23, 0x20, 0xff]);
  writeFileSync(path, bytes);
  expect(
    (
      await call("record_note", {
        path: "notes.md",
        source: "paper.pdf",
        anchor: "page:1",
        note: "new note",
      })
    ).isError,
  ).toBe(true);
  expect(readFileSync(path)).toEqual(bytes);
});

test("note provenance must be re-extracted after a restart", async () => {
  const { call, makePack } = fixture();
  await call("read_pdf", { path: "paper.pdf" });
  const restarted = makePack().tools?.find(
    (tool) => tool.name === "record_note",
  );
  expect(
    (
      await restarted?.execute("fixture", {
        path: "notes.md",
        source: "paper.pdf",
        anchor: "page:1",
        note: "note",
      })
    )?.isError,
  ).toBe(true);
});

test("PDF and note paths reject escapes, symlinks and hard links", async () => {
  const { root, cwd, call } = fixture();
  const outside = join(root, "outside");
  mkdirSync(outside);
  const outsidePdf = join(outside, "paper.pdf");
  copyFileSync(join(cwd, "paper.pdf"), outsidePdf);
  const outsideNote = join(outside, "notes.md");
  writeFileSync(outsideNote, "outside sentinel");
  symlinkSync(outside, join(cwd, "linked"));
  symlinkSync(outsidePdf, join(cwd, "symlink.pdf"));
  linkSync(outsidePdf, join(cwd, "hard.pdf"));
  symlinkSync(outsideNote, join(cwd, "symlink.md"));
  linkSync(outsideNote, join(cwd, "hard.md"));
  await call("read_pdf", { path: "paper.pdf" });
  for (const path of [
    "../outside/paper.pdf",
    outsidePdf,
    "linked/paper.pdf",
    "symlink.pdf",
    "hard.pdf",
  ])
    expect((await call("read_pdf", { path })).isError).toBe(true);
  for (const path of [
    "../outside/notes.md",
    outsideNote,
    "linked/new/notes.md",
    "symlink.md",
    "hard.md",
  ])
    expect(
      (
        await call("record_note", {
          path,
          source: "paper.pdf",
          anchor: "page:1",
          note: "changed",
        })
      ).isError,
    ).toBe(true);
  expect(readFileSync(outsideNote, "utf8")).toBe("outside sentinel");
  expect(existsSync(join(outside, "new"))).toBe(false);
});

test("malformed, oversized and textless PDFs return errors; EPUB is explicitly unavailable", async () => {
  const { cwd, call } = fixture();
  writeFileSync(join(cwd, "invalid.pdf"), "not a PDF");
  writeFileSync(join(cwd, "large.pdf"), "");
  truncateSync(join(cwd, "large.pdf"), MAX_PDF_BYTES + 1);
  const blank = readFileSync(join(cwd, "paper.pdf"), "latin1")
    .replace("First page: anchored evidence.", " ".repeat(30))
    .replace("Second page: another passage.", " ".repeat(29));
  // Length-preserving stream replacement keeps the fixture's PDF offsets valid.
  writeFileSync(join(cwd, "blank.pdf"), blank, "latin1");
  for (const path of ["invalid.pdf", "large.pdf", "book.epub"])
    expect((await call("read_pdf", { path })).isError).toBe(true);
  const empty = await call("read_pdf", { path: "blank.pdf" });
  expect(empty.isError).toBe(true);
  expect(content(empty)).toContain("OCR");
});

test("cancelled tools return errors before reading, fetching or creating notes", async () => {
  const { cwd, call, requests } = fixture();
  await call("read_pdf", { path: "paper.pdf" });
  const signal = AbortSignal.abort(new Error("cancelled"));
  expect((await call("read_pdf", { path: "paper.pdf" }, signal)).isError).toBe(
    true,
  );
  expect(
    (await call("read_article", { url: "https://article.example/" }, signal))
      .isError,
  ).toBe(true);
  expect(
    (
      await call(
        "record_note",
        {
          path: "notes.md",
          source: "paper.pdf",
          anchor: "page:1",
          note: "cancelled",
        },
        signal,
      )
    ).isError,
  ).toBe(true);
  expect(requests()).toBe(0);
  expect(existsSync(join(cwd, "notes.md"))).toBe(false);
});
