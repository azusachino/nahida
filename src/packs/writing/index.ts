import { ftruncateSync, readFileSync, writeFileSync, writeSync } from "node:fs";
import type { AgentTool, AgentToolResult } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import type { Pack } from "../../pack";
import { workspace } from "../../workspace";
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
  const files = workspace(cwd);
  const withDraft = files.withFile;
  const draftPath = (input: string, createParents = false) =>
    files.path(input, [".md", ".markdown"], createParents);

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
