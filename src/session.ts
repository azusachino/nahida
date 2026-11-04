import { createHash } from "node:crypto";
import {
  closeSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
  CURRENT_SESSION_VERSION,
  type FileEntry,
  SessionManager,
} from "@earendil-works/pi-coding-agent";
import { BootError } from "./pack";

/** Hash canonical paths to avoid aliases and collisions in pi's display-oriented cwd encoding. */
export function sessionDir(
  env: Record<string, string | undefined>,
  cwd: string,
): string {
  const root = env.NAHIDA_CONFIG_DIR ?? join(homedir(), ".config", "nahida");
  const key = createHash("sha256").update(realpathSync(cwd)).digest("hex");
  return join(root, "sessions", key);
}

/** Pi intentionally skips corrupt lines. Nahida must reject them before pi opens or repairs a file. */
function validateLog(path: string, cwd: string): void {
  const text = readFileSync(path, "utf8");
  const corrupt = () =>
    new BootError(`corrupt session log: ${path}; left unchanged`);
  if (!text.endsWith("\n")) throw corrupt();
  let entries: FileEntry[];
  try {
    entries = text
      .slice(0, -1)
      .split("\n")
      .map((line) => JSON.parse(line));
  } catch {
    throw corrupt();
  }
  const header = entries[0];
  if (
    header?.type !== "session" ||
    header.version !== CURRENT_SESSION_VERSION ||
    typeof header.id !== "string" ||
    header.cwd !== cwd
  )
    throw corrupt();
  const ids = new Set<string>();
  let parent: string | null = null;
  for (const entry of entries.slice(1)) {
    if (
      !entry ||
      entry.type === "session" ||
      typeof entry.id !== "string" ||
      ids.has(entry.id) ||
      entry.parentId !== parent ||
      typeof entry.timestamp !== "string"
    )
      throw corrupt();
    ids.add(entry.id);
    parent = entry.id;
    if (entry.type === "message") {
      const message = entry.message;
      if (
        !message ||
        (message.role !== "system" &&
          message.role !== "user" &&
          message.role !== "assistant" &&
          message.role !== "toolResult") ||
        !(
          typeof message.content === "string" || Array.isArray(message.content)
        ) ||
        typeof message.timestamp !== "number" ||
        (Array.isArray(message.content) &&
          message.content.some(
            (block) => !block || typeof block.type !== "string",
          ))
      )
        throw corrupt();
      if (
        message.role === "assistant" &&
        (!Array.isArray(message.content) ||
          !["stop", "toolUse"].includes(message.stopReason))
      )
        throw corrupt();
      if (
        message.role === "toolResult" &&
        (typeof message.toolCallId !== "string" ||
          typeof message.toolName !== "string" ||
          !Array.isArray(message.content))
      )
        throw corrupt();
      if (message.role === "assistant") {
        for (const block of message.content) {
          if (
            block.type === "toolCall" &&
            (typeof block.id !== "string" ||
              typeof block.name !== "string" ||
              !block.arguments ||
              typeof block.arguments !== "object" ||
              Array.isArray(block.arguments))
          )
            throw corrupt();
        }
      }
    } else if (entry.type !== "custom" && entry.type !== "model_change") {
      throw corrupt();
    }
  }
}

/** Refuse uncertain side effects instead of retrying a tool after a crash. */
function assertSettledTools(manager: SessionManager): void {
  const pending = new Set<string>();
  for (const message of manager.buildSessionContext().messages) {
    if (message.role === "assistant") {
      if (pending.size)
        throw new BootError(
          "session has unresolved tool calls; inspect side effects before continuing",
        );
      for (const block of message.content) {
        if (block.type === "toolCall") pending.add(block.id);
      }
    } else if (message.role === "toolResult") {
      if (!pending.delete(message.toolCallId))
        throw new BootError("session has an unmatched tool result");
    } else if (pending.size) {
      throw new BootError(
        "session has unresolved tool calls; inspect side effects before continuing",
      );
    }
  }
  if (pending.size)
    throw new BootError(
      "session has unresolved tool calls; inspect side effects before continuing",
    );
}

/** One writer for this workspace until the run and pack disposal settle. */
export function openSession(options: {
  dir: string;
  cwd: string;
  resume: boolean;
  provider: string;
  modelId: string;
  signature: string;
}) {
  const { dir, cwd, resume, provider, modelId, signature } = options;
  mkdirSync(dir, { recursive: true });
  const lock = join(dir, ".lock");
  let fd: number;
  try {
    fd = openSync(lock, "wx", 0o600);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "EEXIST") {
      throw new BootError(
        `session workspace is locked: ${lock}; inspect its owner before removing a stale lock`,
      );
    }
    throw error;
  }
  const release = () => unlinkSync(lock);
  try {
    try {
      writeFileSync(fd, `${process.pid}\n`);
    } finally {
      closeSync(fd);
    }
    // Inspect the newest file even if its header is damaged. Pi's best-effort
    // discovery would skip it, silently selecting an older conversation.
    const latest = resume
      ? readdirSync(dir)
          .filter((name) => name.endsWith(".jsonl"))
          .map((name) => ({
            path: join(dir, name),
            modified: statSync(join(dir, name)).mtimeMs,
          }))
          .sort(
            (a, b) => b.modified - a.modified || b.path.localeCompare(a.path),
          )[0]?.path
      : undefined;
    if (resume && !latest)
      throw new BootError("no session to continue in this workspace");
    if (latest) validateLog(latest, cwd);
    const manager = latest
      ? SessionManager.open(latest, dir, cwd)
      : SessionManager.create(cwd, dir);
    const config = { provider, modelId, signature };
    if (latest) {
      const stored = manager
        .getEntries()
        .find(
          (entry) =>
            entry.type === "custom" && entry.customType === "nahida.config",
        );
      if (
        stored?.type !== "custom" ||
        JSON.stringify(stored.data) !== JSON.stringify(config)
      ) {
        throw new BootError(
          "session model or pack configuration changed; start a new session without --continue",
        );
      }
      assertSettledTools(manager);
    } else {
      manager.appendModelChange(provider, modelId);
      manager.appendCustomEntry("nahida.config", config);
    }
    return { manager, fresh: !latest, release };
  } catch (error) {
    release();
    throw error;
  }
}
