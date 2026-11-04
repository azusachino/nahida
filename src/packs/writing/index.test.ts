import { afterEach, expect, test } from "bun:test";
import {
  existsSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { writingPack } from "./index";

const scratch = resolve(import.meta.dir, "../../../.tmp/writing-tests");
const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0))
    rmSync(root, { recursive: true, force: true });
});

function fixture() {
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, "case-"));
  roots.push(root);
  const cwd = join(root, "workspace");
  const outside = join(root, "workspace-sibling");
  mkdirSync(cwd);
  mkdirSync(outside);
  const pack = writingPack(cwd);
  const call = async (name: string, args: Record<string, unknown>) => {
    const tool = pack.tools?.find((tool) => tool.name === name);
    if (!tool) throw new Error(`missing tool ${name}`);
    return tool.execute("fixture-call", args);
  };
  return { cwd, outside, call };
}

test("creates nested UTF-8 Markdown drafts, reads exact bytes, and refuses overwrite", async () => {
  const { cwd, call } = fixture();
  const text = "# Draft\r\n\r\n春と nahida 🌱\r\n";
  expect(
    (await call("create_draft", { path: "drafts/first.md", text })).isError,
  ).not.toBe(true);
  const path = join(cwd, "drafts/first.md");
  expect(readFileSync(path)).toEqual(Buffer.from(text));
  expect(
    (await call("read_draft", { path: "drafts/first.md" })).content,
  ).toEqual([{ type: "text", text }]);
  expect(
    (await call("create_draft", { path: "drafts/first.md", text: "overwrite" }))
      .isError,
  ).toBe(true);
  expect(readFileSync(path, "utf8")).toBe(text);
});

test("literal edits preserve surrounding bytes, allow shrinking and deletion, and do not expand replacement patterns", async () => {
  const { cwd, call } = fixture();
  const path = join(cwd, "draft.markdown");
  writeFileSync(path, "# 春\r\nold passage\r\nkeep 🌱\r\n");
  expect(
    (
      await call("edit_draft", {
        path,
        oldText: "old passage",
        newText: "$& 日本語",
      })
    ).isError,
  ).not.toBe(true);
  expect(readFileSync(path, "utf8")).toBe("# 春\r\n$& 日本語\r\nkeep 🌱\r\n");
  expect(
    (await call("edit_draft", { path, oldText: "$& 日本語", newText: "x" }))
      .isError,
  ).not.toBe(true);
  expect(readFileSync(path, "utf8")).toBe("# 春\r\nx\r\nkeep 🌱\r\n");
  expect(
    (
      await call("edit_draft", {
        path,
        oldText: "# 春\r\nx\r\nkeep 🌱\r\n",
        newText: "",
      })
    ).isError,
  ).not.toBe(true);
  expect(readFileSync(path, "utf8")).toBe("");
});

test.each([
  ["missing", "a complete draft"],
  ["repeat", "repeat then repeat"],
  ["aa", "aaa"],
  ["", "a complete draft"],
])(
  "refuses missing, ambiguous or empty edit target %j without changing bytes",
  async (oldText, text) => {
    const { cwd, call } = fixture();
    const path = join(cwd, "draft.md");
    writeFileSync(path, text);
    const result = await call("edit_draft", {
      path: "draft.md",
      oldText,
      newText: "replacement",
    });
    expect(result.isError).toBe(true);
    expect(readFileSync(path, "utf8")).toBe(text);
  },
);

test("all tools reject traversal and absolute escapes without modifying outside drafts", async () => {
  const { outside, call } = fixture();
  const path = join(outside, "private.md");
  writeFileSync(path, "outside sentinel");
  for (const candidate of [
    "../workspace-sibling/private.md",
    path,
    "../../escaped.md",
  ]) {
    for (const tool of ["read_draft", "create_draft", "edit_draft"]) {
      const result = await call(tool, {
        path: candidate,
        text: "changed",
        oldText: "outside sentinel",
        newText: "changed",
      });
      expect(result.isError).toBe(true);
      expect(JSON.stringify(result.content)).toContain("inside the workspace");
      expect(JSON.stringify(result.content)).not.toContain("outside sentinel");
    }
  }
  expect(readFileSync(path, "utf8")).toBe("outside sentinel");
});

test("all tools reject symlink files and parents, including dangling final links", async () => {
  const { cwd, outside, call } = fixture();
  writeFileSync(join(outside, "private.md"), "outside sentinel");
  symlinkSync(join(outside, "private.md"), join(cwd, "link.md"));
  symlinkSync(join(outside, "missing.md"), join(cwd, "dangling.md"));
  symlinkSync(outside, join(cwd, "linked-dir"));
  for (const path of [
    "link.md",
    "dangling.md",
    "linked-dir/private.md",
    "linked-dir/new/deep.md",
  ]) {
    for (const tool of ["read_draft", "create_draft", "edit_draft"]) {
      expect(
        (
          await call(tool, {
            path,
            text: "changed",
            oldText: "outside sentinel",
            newText: "changed",
          })
        ).isError,
      ).toBe(true);
    }
  }
  expect(readFileSync(join(outside, "private.md"), "utf8")).toBe(
    "outside sentinel",
  );
  expect(existsSync(join(outside, "missing.md"))).toBe(false);
  expect(existsSync(join(outside, "new"))).toBe(false);
});

test("hard links cannot expose or edit an outside draft", async () => {
  const { cwd, outside, call } = fixture();
  const original = join(outside, "private.md");
  writeFileSync(original, "outside sentinel");
  linkSync(original, join(cwd, "alias.md"));
  for (const tool of ["read_draft", "create_draft", "edit_draft"]) {
    expect(
      (
        await call(tool, {
          path: "alias.md",
          text: "changed",
          oldText: "outside sentinel",
          newText: "changed",
        })
      ).isError,
    ).toBe(true);
  }
  expect(readFileSync(original, "utf8")).toBe("outside sentinel");
});

test("filesystem failures, non-Markdown paths and directories return errors", async () => {
  const { cwd, call } = fixture();
  mkdirSync(join(cwd, "directory.md"));
  writeFileSync(join(cwd, "parent"), "not a directory");
  for (const path of [
    ".",
    "notes.txt",
    "directory.md",
    "parent/draft.md",
    "bad\0.md",
  ]) {
    for (const tool of ["read_draft", "create_draft", "edit_draft"]) {
      expect(
        (
          await call(tool, {
            path,
            text: "new",
            oldText: "old",
            newText: "new",
          })
        ).isError,
      ).toBe(true);
    }
  }
  expect((await call("read_draft", { path: "missing.md" })).isError).toBe(true);
  expect(
    (
      await call("edit_draft", {
        path: "missing.md",
        oldText: "old",
        newText: "new",
      })
    ).isError,
  ).toBe(true);
});

test("preserves UTF-8 BOM and refuses invalid encoding without changing bytes", async () => {
  const { cwd, call } = fixture();
  const path = join(cwd, "draft.md");
  writeFileSync(path, "\uFEFF# Old\r\n");
  expect((await call("read_draft", { path })).content).toEqual([
    { type: "text", text: "\uFEFF# Old\r\n" },
  ]);
  expect(
    (await call("edit_draft", { path, oldText: "Old", newText: "New" }))
      .isError,
  ).not.toBe(true);
  expect(readFileSync(path)).toEqual(Buffer.from("\uFEFF# New\r\n"));
  const invalid = Buffer.from([0x23, 0x20, 0xff, 0x0a]);
  writeFileSync(path, invalid);
  expect((await call("read_draft", { path })).isError).toBe(true);
  expect(
    (await call("edit_draft", { path, oldText: "#", newText: "changed" }))
      .isError,
  ).toBe(true);
  expect(readFileSync(path)).toEqual(invalid);
});

test("pack instances keep independent workspace bindings", async () => {
  const first = fixture();
  const second = fixture();
  expect(
    (await first.call("create_draft", { path: "draft.md", text: "first" }))
      .isError,
  ).not.toBe(true);
  expect(
    (await second.call("create_draft", { path: "draft.md", text: "second" }))
      .isError,
  ).not.toBe(true);
  expect(readFileSync(join(first.cwd, "draft.md"), "utf8")).toBe("first");
  expect(readFileSync(join(second.cwd, "draft.md"), "utf8")).toBe("second");
});
