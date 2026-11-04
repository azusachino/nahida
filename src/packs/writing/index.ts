import {
  closeSync,
  constants,
  fstatSync,
  ftruncateSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  realpathSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import {
  dirname,
  extname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import type { AgentTool, AgentToolResult } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import type { Pack } from "../../pack";
import prompt from "./prompt.md" with { type: "text" };

const utf8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

const pathParameters = Type.Object({ path: Type.String({ minLength: 1 }) });
const createParameters = Type.Object({
  path: Type.String({ minLength: 1 }),
  text: Type.String(),
});
const editParameters = Type.Object({
  path: Type.String({ minLength: 1 }),
  oldText: Type.String({ minLength: 1 }),
  newText: Type.String(),
});

function result(action: () => string): AgentToolResult {
  try {
    return { content: [{ type: "text", text: action() }], details: undefined };
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

/** Each run owns its workspace binding; tool calls cannot choose or mutate it. */
export function writingPack(cwd: string): Pack {
  const root = realpathSync(cwd);

  function draftPath(input: string, createParents = false): string {
    const path = resolve(root, input);
    const local = relative(root, path);
    if (
      !local ||
      local === ".." ||
      local.startsWith(`..${sep}`) ||
      isAbsolute(local)
    ) {
      throw new Error("draft path must be inside the workspace");
    }
    if (![".md", ".markdown"].includes(extname(path).toLowerCase())) {
      throw new Error("draft path must end in .md or .markdown");
    }
    let parent = root;
    for (const part of relative(root, dirname(path))
      .split(sep)
      .filter(Boolean)) {
      parent = join(parent, part);
      if (createParents) {
        try {
          mkdirSync(parent);
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
        }
      }
      const stat = lstatSync(parent);
      if (stat.isSymbolicLink() || !stat.isDirectory()) {
        throw new Error(
          "draft parent must be a real workspace directory; symlinks are not allowed",
        );
      }
    }
    return path;
  }

  function withDraft(
    path: string,
    writable: boolean,
    action: (fd: number) => string,
  ): string {
    const stat = lstatSync(path);
    if (!stat.isFile() || stat.nlink !== 1) {
      throw new Error(
        "draft must be a regular file with one link; symlinks and hard links are not allowed",
      );
    }
    const fd = openSync(
      path,
      (writable ? constants.O_RDWR : constants.O_RDONLY) |
        constants.O_NOFOLLOW |
        constants.O_NONBLOCK,
    );
    try {
      const opened = fstatSync(fd);
      if (
        !opened.isFile() ||
        opened.nlink !== 1 ||
        opened.dev !== stat.dev ||
        opened.ino !== stat.ino
      ) {
        throw new Error(
          "draft changed while opening; retry after checking the workspace",
        );
      }
      return action(fd);
    } finally {
      closeSync(fd);
    }
  }

  const readTool: AgentTool<typeof pathParameters> = {
    name: "read_draft",
    label: "Read draft",
    description: "Read a Markdown draft inside the workspace.",
    parameters: pathParameters,
    executionMode: "sequential",
    execute: async (_id, { path }) =>
      result(() =>
        withDraft(draftPath(path), false, (fd) =>
          utf8.decode(readFileSync(fd)),
        ),
      ),
  };
  const createTool: AgentTool<typeof createParameters> = {
    name: "create_draft",
    label: "Create draft",
    description:
      "Create a new Markdown draft inside the workspace. Creates parent directories; never overwrites an existing path.",
    parameters: createParameters,
    executionMode: "sequential",
    execute: async (_id, { path, text }) =>
      result(() => {
        writeFileSync(draftPath(path, true), text, {
          encoding: "utf8",
          flag: "wx",
          mode: 0o600,
        });
        return `Created ${path}`;
      }),
  };
  const editTool: AgentTool<typeof editParameters> = {
    name: "edit_draft",
    label: "Edit draft",
    description:
      "Replace exactly one literal oldText occurrence in a workspace Markdown draft. Refuses empty, missing or ambiguous targets.",
    parameters: editParameters,
    executionMode: "sequential",
    execute: async (_id, { path, oldText, newText }) =>
      result(() => {
        if (!oldText) throw new Error("oldText must not be empty");
        return withDraft(draftPath(path), true, (fd) => {
          const text = utf8.decode(readFileSync(fd));
          const start = text.indexOf(oldText);
          if (start < 0)
            throw new Error(
              "oldText was not found; read the draft before editing",
            );
          if (text.indexOf(oldText, start + 1) !== -1)
            throw new Error(
              "oldText is ambiguous; include more surrounding text",
            );
          const updated = Buffer.from(
            text.slice(0, start) + newText + text.slice(start + oldText.length),
            "utf8",
          );
          let offset = 0;
          while (offset < updated.length) {
            const written = writeSync(
              fd,
              updated,
              offset,
              updated.length - offset,
              offset,
            );
            if (written === 0)
              throw new Error("could not finish writing the draft");
            offset += written;
          }
          ftruncateSync(fd, updated.length);
          return `Edited ${path}`;
        });
      }),
  };
  return {
    id: "writing",
    prompt: { file: "prompt.md", text: prompt },
    tools: [readTool, createTool, editTool],
  };
}
