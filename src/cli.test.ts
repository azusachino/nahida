import { afterEach, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import {
  type Api,
  createAssistantMessageEventStream,
  fauxAssistantMessage,
  fauxProvider,
  fauxToolCall,
  type Model,
  normalizeContext,
} from "@earendil-works/pi-ai";
import { SessionManager } from "@earendil-works/pi-coding-agent";
import { type Io, main } from "./cli";
import type { Pack, PackRegistry, PackTool } from "./pack";
import { echoPack, echoTool } from "./packs/echo";
import { piAgentDir } from "./pi-config";
import { openSession, sessionDir } from "./session";

const SCRATCH = resolve(import.meta.dir, "../.tmp/cli-tests");
const SENTINEL_AUTH = "sk-sentinel-auth-json-7f3a";
const SENTINEL_MODELS = "sk-sentinel-models-json-91c2";

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0))
    rmSync(root, { recursive: true, force: true });
});

/** A throwaway pi agent dir and nahida config dir. Nothing outside SCRATCH is touched. */
function fixture(
  options: {
    settings?: Record<string, unknown>;
    profiles?: Record<string, string>;
  } = {},
) {
  mkdirSync(SCRATCH, { recursive: true });
  const root = mkdtempSync(join(SCRATCH, "case-"));
  roots.push(root);
  const cwd = join(root, "workspace");
  mkdirSync(cwd);
  const agentDir = join(root, "agent");
  const profilesDir = join(root, "config", "profiles");
  mkdirSync(agentDir, { recursive: true });
  mkdirSync(profilesDir, { recursive: true });
  const write = (name: string, value: unknown) =>
    writeFileSync(join(agentDir, name), JSON.stringify(value, null, 2));
  write("auth.json", {
    fake: { type: "api_key", key: SENTINEL_AUTH },
  });
  write("models.json", {
    providers: {
      fake: {
        baseUrl: "http://127.0.0.1:9/v1",
        api: "openai-completions",
        models: [{ id: "fake-model" }],
      },
      "zai-coding-cn": {
        baseUrl: "http://127.0.0.1:9/v1",
        api: "openai-completions",
        apiKey: SENTINEL_MODELS,
        models: [{ id: "glm-x" }],
      },
      nokey: {
        baseUrl: "http://127.0.0.1:9/v1",
        api: "openai-completions",
        models: [{ id: "nokey-model" }],
      },
    },
  });
  write(
    "settings.json",
    options.settings ?? {
      defaultProvider: "fake",
      defaultModel: "fake-model",
    },
  );
  for (const [name, text] of Object.entries(options.profiles ?? {})) {
    writeFileSync(join(profilesDir, `${name}.toml`), text);
  }
  return {
    cwd,
    agentDir,
    env: {
      PI_CODING_AGENT_DIR: agentDir,
      NAHIDA_CONFIG_DIR: join(root, "config"),
    } as Record<string, string | undefined>,
  };
}

type Responses = Parameters<ReturnType<typeof fauxProvider>["setResponses"]>[0];

/** Captures what the CLI writes and routes model calls to a scripted faux provider. */
function harness(
  env: Record<string, string | undefined>,
  options: { responses?: Responses; packs?: PackRegistry } = {},
) {
  const faux = fauxProvider();
  faux.setResponses(options.responses ?? []);
  const stdout: string[] = [];
  const stderr: string[] = [];
  const calls: { model: Model<Api>; context: unknown }[] = [];
  const io: Io = {
    env,
    cwd: resolve(env.NAHIDA_CONFIG_DIR as string, "../workspace"),
    stdout: (text) => stdout.push(text),
    stderr: (text) => stderr.push(text),
    packs: options.packs,
    streamFn: (model, context, opts) => {
      calls.push({ model, context: structuredClone(context) });
      return faux.provider.streamSimple(model, context, opts);
    },
  };
  return { io, faux, stdout, stderr, calls };
}

/** Tool names declared to the model in a transcript, read from its system messages. */
function toolNamesIn(context: unknown): string[] {
  const text = JSON.stringify(context);
  return ["echo", "other_tool"].filter((name) => text.includes(`"${name}"`));
}

/** A pack that records activation and disposal in the shared log. */
function recordingPack(id: string, log: string[]): Pack {
  return {
    id,
    activate: () => {
      log.push(`activate:${id}`);
      return () => {
        log.push(`dispose:${id}`);
      };
    },
  };
}

describe("missing credentials", () => {
  test("exits 1, names the provider, and makes no model call", async () => {
    const { env } = fixture({
      settings: { defaultProvider: "nokey", defaultModel: "nokey-model" },
    });
    const { io, faux, stderr } = harness(env);
    const code = await main(["hello"], io);

    expect(code).toBe(1);
    expect(stderr.join("")).toContain("no credentials for provider nokey");
    expect(faux.state.callCount).toBe(0);
  });
});

describe("--json", () => {
  test("emits one JSON object per line, each parseable", async () => {
    const { env } = fixture();
    const { io, stdout } = harness(env, {
      responses: [fauxAssistantMessage("Hello there.")],
    });
    const code = await main(["--json", "say hi"], io);

    expect(code).toBe(0);
    const lines = stdout
      .join("")
      .split("\n")
      .filter((line) => line.length > 0);
    expect(lines.length).toBeGreaterThan(1);
    const types = lines.map((line) => JSON.parse(line).type);
    expect(types).toContain("agent_start");
    expect(types).toContain("message_update");
    expect(types.at(-1)).toBe("agent_end");
  });
});

describe("text output", () => {
  test("streams the assistant text to stdout", async () => {
    const { env } = fixture();
    const { io, stdout } = harness(env, {
      responses: [fauxAssistantMessage("Hello there.")],
    });
    const code = await main(["say hi"], io);

    expect(code).toBe(0);
    expect(stdout.join("")).toContain("Hello there.");
  });

  test("a model error exits 1 and reports the message on stderr", async () => {
    const { env } = fixture();
    const { io, stderr } = harness(env, {
      responses: [
        fauxAssistantMessage("", {
          stopReason: "error",
          errorMessage: "upstream exploded",
        }),
      ],
    });
    const code = await main(["say hi"], io);

    expect(code).toBe(1);
    expect(stderr.join("")).toContain("upstream exploded");
  });
});

describe("packs and profiles", () => {
  test("two packs registering the same tool name fail at boot, naming both", async () => {
    const twin: Pack = { id: "twin", tools: [echoTool] };
    const { env } = fixture({
      profiles: { both: 'packs = ["echo", "twin"]\n' },
    });
    const { io, faux, stderr } = harness(env, {
      packs: { echo: echoPack, twin },
    });
    const code = await main(["--profile", "both", "hi"], io);

    expect(code).toBe(2);
    const message = stderr.join("");
    expect(message).toContain('"echo"');
    expect(message).toContain('"twin"');
    expect(faux.state.callCount).toBe(0);
  });

  test("--profile exposes only that profile's tools in the request the model receives", async () => {
    const other: Pack = {
      id: "other",
      tools: [{ ...echoTool, name: "other_tool" } as PackTool],
    };
    const { env } = fixture({
      profiles: { "other-only": 'packs = ["other"]\n' },
    });
    const registry = { echo: echoPack, other };
    const scoped = harness(env, {
      packs: registry,
      responses: [fauxAssistantMessage("ok")],
    });
    expect(await main(["--profile", "other-only", "hi"], scoped.io)).toBe(0);

    expect(scoped.calls).toHaveLength(1);
    expect(toolNamesIn(scoped.calls[0]?.context)).toEqual(["other_tool"]);

    const all = harness(env, {
      packs: registry,
      responses: [fauxAssistantMessage("ok")],
    });
    expect(await main(["hi"], all.io)).toBe(0);
    expect(toolNamesIn(all.calls[0]?.context)).toEqual(["echo", "other_tool"]);
  });

  test("an unknown pack id is a boot error", async () => {
    const { env } = fixture({
      profiles: { bad: 'packs = ["nope"]\n' },
    });
    const { io, stderr, faux } = harness(env);
    const code = await main(["--profile", "bad", "hi"], io);

    expect(code).toBe(2);
    expect(stderr.join("")).toContain('unknown pack "nope"');
    expect(faux.state.callCount).toBe(0);
  });

  test("an unknown profile key is a boot error", async () => {
    const { env } = fixture({
      profiles: { typo: 'packs = ["echo"]\npacks_typo = true\n' },
    });
    const { io, stderr } = harness(env);
    const code = await main(["--profile", "typo", "hi"], io);

    expect(code).toBe(2);
    expect(stderr.join("")).toContain('unknown key "packs_typo"');
  });

  test("an unknown profile name is a boot error", async () => {
    const { env } = fixture();
    const { io, stderr } = harness(env);
    const code = await main(["--profile", "missing", "hi"], io);

    expect(code).toBe(2);
    expect(stderr.join("")).toContain('profile "missing" not found');
  });
});

describe("disposal", () => {
  test("disposers run in reverse activation order on normal exit", async () => {
    const log: string[] = [];
    const registry = {
      a: recordingPack("a", log),
      b: recordingPack("b", log),
    };
    const { env } = fixture({
      profiles: { ab: 'packs = ["a", "b"]\n' },
    });
    const { io } = harness(env, {
      packs: registry,
      responses: [fauxAssistantMessage("ok")],
    });
    const code = await main(["--profile", "ab", "hi"], io);

    expect(code).toBe(0);
    expect(log).toEqual(["activate:a", "activate:b", "dispose:b", "dispose:a"]);
  });

  test("SIGINT aborts the run, disposes packs, and exits 130", async () => {
    const log: string[] = [];
    const { env } = fixture({
      profiles: { a: 'packs = ["a"]\n' },
    });
    const faux = fauxProvider();
    faux.setResponses([
      fauxAssistantMessage("", {
        stopReason: "aborted",
        errorMessage: "aborted",
      }),
    ]);
    let requestStarted!: () => void;
    const started = new Promise<void>((resolve) => {
      requestStarted = resolve;
    });
    const io: Io = {
      env,
      stdout: () => {},
      stderr: () => {},
      packs: { a: recordingPack("a", log) },
      // Holds the model call until the run is aborted, as a slow provider would.
      streamFn: (model, context, opts) => {
        requestStarted();
        return new Promise((resolve) => {
          opts?.signal?.addEventListener("abort", () =>
            resolve(faux.provider.streamSimple(model, context, opts)),
          );
        });
      },
    };
    // Without a listener, emitting SIGINT would kill the test runner. The guard keeps a broken handler a plain failure.
    const guard = () => {};
    process.on("SIGINT", guard);
    try {
      const run = main(["--profile", "a", "hi"], io);
      await started;
      process.emit("SIGINT");

      expect(await run).toBe(130);
      expect(log).toEqual(["activate:a", "dispose:a"]);
    } finally {
      process.off("SIGINT", guard);
    }
  });
});

describe("--describe", () => {
  test("prints the resolved profile and never a key, even one set in the environment", async () => {
    const { env, agentDir } = fixture({
      profiles: {
        zai: 'packs = ["echo"]\nmodel = "zai-coding-cn/glm-x"\napproval = "never"\n',
      },
    });
    const { io, stdout, calls } = harness({
      ...env,
      FAKE_PROVIDER_KEY: SENTINEL_AUTH,
    });
    const code = await main(["--describe", "--profile", "zai"], io);
    const text = stdout.join("");

    expect(code).toBe(0);
    expect(text).toContain("profile: zai");
    expect(text).toContain("model: zai-coding-cn/glm-x");
    expect(text).toContain("packs: echo");
    expect(text).toContain("tools: echo");
    expect(text).toContain("prompt: echo (prompt.md)");
    expect(text).toContain("approval: never");
    expect(text).toContain(`agent dir: ${agentDir}`);
    expect(text).not.toContain(SENTINEL_AUTH);
    expect(text).not.toContain(SENTINEL_MODELS);
    expect(calls).toHaveLength(0);
  });
});

describe("model resolution", () => {
  test("the profile's model wins over settings.json defaults", async () => {
    const { env } = fixture({
      profiles: { zai: 'packs = ["echo"]\nmodel = "zai-coding-cn/glm-x"\n' },
    });
    const { io, calls } = harness(env, {
      responses: [fauxAssistantMessage("ok")],
    });
    expect(await main(["--profile", "zai", "hi"], io)).toBe(0);

    expect(calls[0]?.model.provider).toBe("zai-coding-cn");
    expect(calls[0]?.model.id).toBe("glm-x");
  });

  test("settings.json defaults apply when the profile names no model", async () => {
    const { env } = fixture();
    const { io, calls } = harness(env, {
      responses: [fauxAssistantMessage("ok")],
    });
    expect(await main(["hi"], io)).toBe(0);

    expect(calls[0]?.model.provider).toBe("fake");
    expect(calls[0]?.model.id).toBe("fake-model");
  });

  test("no model in the profile or settings is a boot error", async () => {
    const { env } = fixture({ settings: {} });
    const { io, stderr } = harness(env);
    const code = await main(["hi"], io);

    expect(code).toBe(2);
    expect(stderr.join("")).toContain("no model");
  });
});

describe("agent dir", () => {
  test("PI_CODING_AGENT_DIR wins, and ~/.pi/agent is the fallback", async () => {
    expect(piAgentDir({ PI_CODING_AGENT_DIR: "/x/agent" })).toBe("/x/agent");
    expect(piAgentDir({})).toBe(join(homedir(), ".pi", "agent"));
  });
});

/** The only JSONL in a fixture's canonical workspace directory. */
function sessionFile(io: Io): string {
  const dir = sessionDir(io.env, io.cwd ?? process.cwd());
  const files = readdirSync(dir).filter((name) => name.endsWith(".jsonl"));
  expect(files).toHaveLength(1);
  return join(dir, files[0] as string);
}

function replay(path: string) {
  return SessionManager.open(path).buildSessionContext().messages;
}

function requestFromLog(path: string) {
  const messages = replay(path).filter(
    (message) =>
      message.role === "system" ||
      message.role === "user" ||
      message.role === "assistant" ||
      message.role === "toolResult",
  );
  return normalizeContext({ messages });
}

describe("writing pack", () => {
  test("writing-only CLI journey creates, reads and edits a draft, recovers from a tool error and resumes", async () => {
    const { env, cwd } = fixture();
    const path = join(cwd, "drafts/story.md");
    const h = harness(env, {
      responses: [
        fauxAssistantMessage(
          fauxToolCall("create_draft", {
            path: "drafts/story.md",
            text: "# 春\nAn early draft.\n",
          }),
          { stopReason: "toolUse" },
        ),
        () => {
          expect(readFileSync(path, "utf8")).toBe("# 春\nAn early draft.\n");
          return fauxAssistantMessage(
            fauxToolCall("read_draft", { path: "drafts/story.md" }),
            { stopReason: "toolUse" },
          );
        },
        fauxAssistantMessage(
          fauxToolCall("edit_draft", {
            path: "drafts/story.md",
            oldText: "absent passage",
            newText: "unsafe",
          }),
          { stopReason: "toolUse" },
        ),
        () => {
          expect(readFileSync(path, "utf8")).toBe("# 春\nAn early draft.\n");
          const messages = replay(sessionFile(h.io));
          expect(messages.at(-1)?.role).toBe("toolResult");
          const last = messages.at(-1);
          expect(last?.role === "toolResult" && last.isError).toBe(true);
          return fauxAssistantMessage(
            fauxToolCall("edit_draft", {
              path: "drafts/story.md",
              oldText: "An early",
              newText: "A finished",
            }),
            { stopReason: "toolUse" },
          );
        },
        fauxAssistantMessage("Draft saved."),
      ],
    });
    expect(await main(["--profile", "writing", "write a draft"], h.io)).toBe(0);
    expect(readFileSync(path)).toEqual(
      Buffer.from("# 春\nA finished draft.\n"),
    );
    const first = h.calls[0]?.context as {
      messages: { toolsAdded?: { name: string }[] }[];
    };
    expect(first.messages[0]?.toolsAdded?.map((tool) => tool.name)).toEqual([
      "read_draft",
      "create_draft",
      "edit_draft",
    ]);
    expect(h.stdout.join("")).toContain("Draft saved.");
    const next = harness(env, {
      responses: [fauxAssistantMessage("Remembered the draft.")],
    });
    expect(
      await main(
        ["--continue", "--profile", "writing", "what did you write?"],
        next.io,
      ),
    ).toBe(0);
    expect(JSON.stringify(next.calls)).toContain("# 春\\nAn early draft.");
    expect(sessionFile(next.io)).toBe(sessionFile(h.io));
  });

  test("escaped paths become model-visible tool errors and never touch the outside draft", async () => {
    const { env, cwd } = fixture();
    const outside = join(cwd, "../outside.md");
    writeFileSync(outside, "outside sentinel");
    const h = harness(env, {
      responses: [
        fauxAssistantMessage(
          fauxToolCall("edit_draft", {
            path: "../outside.md",
            oldText: "outside sentinel",
            newText: "changed",
          }),
          { stopReason: "toolUse" },
        ),
        () => {
          const last = replay(sessionFile(h.io)).at(-1);
          expect(last?.role === "toolResult" && last.isError).toBe(true);
          expect(JSON.stringify(last)).toContain("inside the workspace");
          return fauxAssistantMessage("That path is outside the workspace.");
        },
      ],
    });
    expect(await main(["--profile", "writing", "edit it"], h.io)).toBe(0);
    expect(readFileSync(outside, "utf8")).toBe("outside sentinel");
  });

  test("echo-only sessions can keep their original configuration after the default gains writing", async () => {
    const { env } = fixture({
      profiles: { "echo-only": 'packs = ["echo"]\n' },
    });
    const previous = harness(env, {
      packs: { echo: echoPack },
      responses: [fauxAssistantMessage("old session")],
    });
    expect(await main(["hi"], previous.io)).toBe(0);
    const changed = harness(env);
    expect(await main(["--continue", "more"], changed.io)).toBe(2);
    expect(changed.stderr.join("")).toContain("configuration changed");
    const compatible = harness(env, {
      responses: [fauxAssistantMessage("continued")],
    });
    expect(
      await main(
        ["--continue", "--profile", "echo-only", "more"],
        compatible.io,
      ),
    ).toBe(0);
  });

  test("--describe resolves the built-in writing-only profile", async () => {
    const { env } = fixture();
    const h = harness(env);
    expect(await main(["--describe", "--profile", "writing"], h.io)).toBe(0);
    expect(h.stdout.join("")).toContain("packs: writing\n");
    expect(h.stdout.join("")).toContain(
      "tools: read_draft, create_draft, edit_draft\n",
    );
    expect(h.calls).toHaveLength(0);
  });

  test("an explicit profile overrides the built-in writing profile", async () => {
    const { env } = fixture({ profiles: { writing: 'packs = ["echo"]\n' } });
    const h = harness(env);
    expect(await main(["--describe", "--profile", "writing"], h.io)).toBe(0);
    expect(h.stdout.join("")).toContain("tools: echo\n");
  });
});

describe("sessions", () => {
  test("restart replays prompt, tools, reasoning signatures and tool results exactly", async () => {
    const { env } = fixture();
    const first = harness(env, {
      responses: [
        fauxAssistantMessage(
          [
            {
              type: "thinking",
              thinking: "remember",
              thinkingSignature: "opaque-signature",
            },
            fauxToolCall("echo", { text: "remember me" }, { id: "echo-1" }),
          ],
          { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("remembered"),
      ],
    });
    expect(await main(["remember this"], first.io)).toBe(0);
    const path = sessionFile(first.io);
    const stored = replay(path);
    // The second request occurred after the echo result was durably written.
    expect(
      (first.calls[1]?.context as { messages: unknown[] } | undefined)
        ?.messages,
    ).toEqual(stored.slice(0, -1));
    expect(JSON.stringify(stored)).toContain("opaque-signature");
    const next = harness(env, {
      responses: [
        () => {
          // Called before the new response is appended: disk is the canonical request.
          expect(next.calls[0]?.context).toEqual(requestFromLog(path));
          return fauxAssistantMessage("continued");
        },
      ],
    });
    expect(await main(["--continue", "what did I say?"], next.io)).toBe(0);
    const request = next.calls[0]?.context as { messages: unknown[] };
    expect(request.messages.slice(0, -1)).toEqual(stored);
    expect(sessionFile(next.io)).toBe(path);
  });

  test("pack context is on disk before the request that consumes it", async () => {
    const { env } = fixture();
    const pack: Pack = {
      id: "passage",
      activate: (ctx) => {
        ctx.addContext("Page 7: a quoted passage");
        return () => {};
      },
    };
    const h = harness(env, {
      packs: { passage: pack },
      responses: [
        () => {
          const path = sessionFile(h.io);
          expect(h.calls[0]?.context).toEqual(requestFromLog(path));
          expect(readFileSync(path, "utf8")).toContain(
            "Page 7: a quoted passage",
          );
          return fauxAssistantMessage("read");
        },
      ],
    });
    expect(await main(["read it"], h.io)).toBe(0);
  });

  test("context added by a tool follows its recorded result and survives continuation", async () => {
    const { env } = fixture();
    let addContext!: (text: string) => void;
    const pack: Pack = {
      id: "passage",
      activate: (ctx) => {
        addContext = ctx.addContext;
        return () => {};
      },
      tools: [
        {
          ...echoTool,
          execute: async () => {
            addContext("Page 8: tool-supplied passage");
            return {
              content: [{ type: "text", text: "extracted" }],
              details: {},
            };
          },
        },
      ],
    };
    const first = harness(env, {
      packs: { passage: pack },
      responses: [
        fauxAssistantMessage(fauxToolCall("echo", { text: "extract" }), {
          stopReason: "toolUse",
        }),
        () => {
          const path = sessionFile(first.io);
          expect(first.calls[1]?.context).toEqual(requestFromLog(path));
          expect(replay(path).map((m) => m.role)).toEqual([
            "system",
            "user",
            "assistant",
            "toolResult",
            "user",
          ]);
          return fauxAssistantMessage("read");
        },
      ],
    });
    expect(await main(["extract it"], first.io)).toBe(0);
    const next = harness(env, {
      packs: { passage: pack },
      responses: [fauxAssistantMessage("continued")],
    });
    expect(await main(["--continue", "more"], next.io)).toBe(0);
    expect(JSON.stringify(next.calls)).toContain(
      "Page 8: tool-supplied passage",
    );
  });

  test("--describe shows the session directory without creating it", async () => {
    const { env, cwd } = fixture();
    const h = harness(env);
    expect(await main(["--describe"], h.io)).toBe(0);
    const dir = sessionDir(env, cwd);
    expect(h.stdout.join("")).toContain(`session dir: ${dir}`);
    expect(existsSync(dir)).toBe(false);
  });

  test("--continue without a prompt retries failed input without replaying partial calls", async () => {
    const { env } = fixture();
    const first = harness(env, {
      responses: [
        fauxAssistantMessage(
          [fauxToolCall("echo", { text: "do not execute" })],
          { stopReason: "error" },
        ),
      ],
    });
    expect(await main(["hi"], first.io)).toBe(1);
    const path = sessionFile(first.io);
    expect(replay(path).map((m) => m.role)).toEqual(["system", "user"]);
    const next = harness(env, {
      responses: [
        () => {
          expect(next.calls[0]?.context).toEqual(requestFromLog(path));
          return fauxAssistantMessage("retry succeeded");
        },
      ],
    });
    expect(await main(["--continue"], next.io)).toBe(0);
    expect(replay(path).filter((m) => m.role === "user")).toHaveLength(1);
  });

  test("a completed session requires new input; an empty workspace cannot continue", async () => {
    const { env } = fixture();
    const empty = harness(env);
    expect(await main(["--continue"], empty.io)).toBe(2);
    expect(empty.stderr.join("")).toContain("no session to continue");
    const first = harness(env, { responses: [fauxAssistantMessage("done")] });
    expect(await main(["hi"], first.io)).toBe(0);
    const path = sessionFile(first.io);
    const before = readFileSync(path, "utf8");
    const next = harness(env);
    expect(await main(["--continue"], next.io)).toBe(2);
    expect(next.stderr.join("")).toContain("no unfinished input");
    expect(readFileSync(path, "utf8")).toBe(before);
    expect(next.calls).toHaveLength(0);
  });

  test("continuation refuses model and pack changes before activation or append", async () => {
    const { env } = fixture({
      profiles: {
        other: 'packs = ["echo"]\nmodel = "zai-coding-cn/glm-x"\n',
        empty: "packs = []\n",
      },
    });
    const first = harness(env, { responses: [fauxAssistantMessage("ok")] });
    expect(await main(["hi"], first.io)).toBe(0);
    const path = sessionFile(first.io);
    const before = readFileSync(path, "utf8");
    for (const profile of ["other", "empty"]) {
      const next = harness(env);
      expect(
        await main(["--continue", "--profile", profile, "hi"], next.io),
      ).toBe(2);
      expect(next.stderr.join("")).toContain(
        "model or pack configuration changed",
      );
      expect(next.calls).toHaveLength(0);
      expect(readFileSync(path, "utf8")).toBe(before);
    }
  });

  test("continuation selects the newest log and refuses its corruption without falling back", async () => {
    const { env, cwd } = fixture();
    const first = harness(env, {
      responses: [fauxAssistantMessage("old answer")],
    });
    expect(await main(["old input"], first.io)).toBe(0);
    const oldPath = sessionFile(first.io);
    utimesSync(oldPath, new Date(0), new Date(0));
    const second = harness(env, {
      responses: [fauxAssistantMessage("new answer")],
    });
    expect(await main(["new input"], second.io)).toBe(0);
    const dir = sessionDir(env, cwd);
    const newPath = readdirSync(dir)
      .filter((name) => name.endsWith(".jsonl"))
      .map((name) => join(dir, name))
      .find((path) => path !== oldPath) as string;
    const next = harness(env, {
      responses: [fauxAssistantMessage("continued")],
    });
    expect(await main(["--continue", "more"], next.io)).toBe(0);
    expect(JSON.stringify(next.calls)).toContain("new input");
    expect(JSON.stringify(next.calls)).not.toContain("old input");
    writeFileSync(newPath, "corrupt header\n");
    const broken = harness(env);
    expect(await main(["--continue", "more"], broken.io)).toBe(2);
    expect(broken.stderr.join("")).toContain("corrupt session log");
    expect(broken.calls).toHaveLength(0);
  });

  test("sessions are isolated by canonical workspace, even with a shared config", async () => {
    const { env, cwd } = fixture();
    const first = harness(env, { responses: [fauxAssistantMessage("ok")] });
    expect(await main(["private to first"], first.io)).toBe(0);
    const other = join(cwd, "other");
    mkdirSync(other);
    const next = harness(env);
    next.io.cwd = other;
    expect(await main(["--continue", "hi"], next.io)).toBe(2);
    expect(next.calls).toHaveLength(0);
  });

  test.each([
    "truncated",
    "invalid-json",
    "broken-chain",
    "empty",
    "bad-header",
  ])("refuses %s log without modifying it", async (kind) => {
    const { env } = fixture();
    const first = harness(env, { responses: [fauxAssistantMessage("ok")] });
    expect(await main(["hi"], first.io)).toBe(0);
    const path = sessionFile(first.io);
    const original = readFileSync(path, "utf8");
    const lines = original.trimEnd().split("\n");
    let broken: string;
    if (kind === "truncated") broken = `${original}{"type":"message"`;
    else if (kind === "invalid-json") broken = `${original}garbage\n`;
    else if (kind === "empty") broken = "";
    else if (kind === "bad-header")
      broken = `{}\n${lines.slice(1).join("\n")}\n`;
    else {
      const last = JSON.parse(lines.pop() as string);
      last.parentId = "missing";
      broken = `${lines.join("\n")}\n${JSON.stringify(last)}\n`;
    }
    writeFileSync(path, broken);
    const next = harness(env);
    expect(await main(["--continue", "hi"], next.io)).toBe(2);
    expect(next.stderr.join("")).toContain("corrupt session log");
    expect(readFileSync(path, "utf8")).toBe(broken);
    expect(next.calls).toHaveLength(0);
  });

  test.each(["error", "aborted", "length", "pending"] as const)(
    "%s responses never execute calls or exit successfully",
    async (stopReason) => {
      let executions = 0;
      const tool = {
        ...echoTool,
        execute: async () => {
          executions++;
          return {
            content: [{ type: "text" as const, text: "executed" }],
            details: {},
          };
        },
      };
      const { env } = fixture();
      const h = harness(env, {
        packs: { echo: { id: "echo", tools: [tool] } },
        responses: [
          fauxAssistantMessage(fauxToolCall("echo", { text: "unsafe" }), {
            stopReason,
          }),
        ],
      });
      expect(await main(["hi"], h.io)).toBe(1);
      expect(executions).toBe(0);
      expect(replay(sessionFile(h.io)).map((m) => m.role)).toEqual([
        "system",
        "user",
      ]);
    },
  );

  test("a stream that fails after partial tool arguments cannot execute or replay them", async () => {
    const { env } = fixture();
    let executions = 0;
    const pack: Pack = {
      id: "echo",
      tools: [
        {
          ...echoTool,
          execute: async () => {
            executions++;
            return { content: [], details: {} };
          },
        },
      ],
    };
    const h = harness(env, { packs: { echo: pack } });
    h.io.streamFn = () => {
      const stream = createAssistantMessageEventStream();
      const partial = fauxAssistantMessage(
        fauxToolCall("echo", { text: "partial" }),
        { stopReason: "pending" },
      );
      stream.push({ type: "start", partial });
      stream.push({
        type: "toolcall_delta",
        contentIndex: 0,
        delta: '{"text":"partial',
        partial,
      });
      stream.push({
        type: "error",
        reason: "error",
        error: {
          ...partial,
          stopReason: "error",
          errorMessage: "connection lost",
        },
      });
      stream.end();
      return stream;
    };
    expect(await main(["hi"], h.io)).toBe(1);
    expect(executions).toBe(0);
    expect(h.stderr.join("")).toContain("connection lost");
    expect(replay(sessionFile(h.io)).map((m) => m.role)).toEqual([
      "system",
      "user",
    ]);
  });

  test("SIGINT during a request leaves retriable input and no partial tool call", async () => {
    const { env } = fixture();
    const first = harness(env);
    const faux = fauxProvider();
    faux.setResponses([
      fauxAssistantMessage(fauxToolCall("echo", { text: "unsafe" }), {
        stopReason: "aborted",
      }),
    ]);
    let started!: () => void;
    const ready = new Promise<void>((resolve) => {
      started = resolve;
    });
    first.io.streamFn = (model, context, opts) => {
      started();
      return new Promise((resolve) =>
        opts?.signal?.addEventListener(
          "abort",
          () => resolve(faux.provider.streamSimple(model, context, opts)),
          { once: true },
        ),
      );
    };
    const running = main(["hi"], first.io);
    await ready;
    process.emit("SIGINT");
    expect(await running).toBe(130);
    const next = harness(env, {
      responses: [fauxAssistantMessage("recovered")],
    });
    expect(await main(["--continue"], next.io)).toBe(0);
    expect(JSON.stringify(next.calls)).not.toContain("unsafe");
  });

  test("SIGINT during a side-effecting tool records its result and continuation does not execute it again", async () => {
    const { env } = fixture();
    let executions = 0;
    const pack: Pack = {
      id: "effect",
      tools: [
        {
          ...echoTool,
          execute: async (_id, _args, signal) => {
            executions++;
            process.emit("SIGINT");
            expect(signal?.aborted).toBe(true);
            return {
              content: [{ type: "text", text: "side effect completed" }],
              details: {},
            };
          },
        },
      ],
    };
    const first = harness(env, {
      packs: { effect: pack },
      responses: [
        fauxAssistantMessage(
          fauxToolCall("echo", { text: "effect" }, { id: "effect-1" }),
          { stopReason: "toolUse" },
        ),
      ],
    });
    expect(await main(["do it"], first.io)).toBe(130);
    expect(first.calls).toHaveLength(1);
    expect(replay(sessionFile(first.io)).at(-1)?.role).toBe("toolResult");
    const next = harness(env, {
      packs: { effect: pack },
      responses: [fauxAssistantMessage("settled")],
    });
    expect(await main(["--continue"], next.io)).toBe(0);
    expect(executions).toBe(1);
    expect(JSON.stringify(next.calls)).toContain("side effect completed");
  });

  test("a crash between a tool call and its result blocks continuation", async () => {
    const { env } = fixture();
    const first = harness(env, { responses: [fauxAssistantMessage("ok")] });
    expect(await main(["hi"], first.io)).toBe(0);
    const path = sessionFile(first.io);
    const manager = SessionManager.open(path);
    manager.appendMessage(
      fauxAssistantMessage(
        fauxToolCall("echo", { text: "maybe executed" }, { id: "uncertain" }),
        { stopReason: "toolUse" },
      ),
    );
    const before = readFileSync(path, "utf8");
    const next = harness(env);
    expect(await main(["--continue", "hi"], next.io)).toBe(2);
    expect(next.stderr.join("")).toContain("unresolved tool calls");
    expect(readFileSync(path, "utf8")).toBe(before);
    expect(next.calls).toHaveLength(0);
  });

  test("exclusive workspace lock blocks another writer and is released after failure", async () => {
    const { env, cwd } = fixture();
    const dir = sessionDir(env, cwd);
    const first = openSession({
      dir,
      cwd,
      resume: false,
      provider: "fake",
      modelId: "fake-model",
      signature: "fixture",
    });
    const next = harness(env);
    expect(await main(["hi"], next.io)).toBe(2);
    expect(next.stderr.join("")).toContain("workspace is locked");
    first.release();
    const retry = harness(env, {
      responses: [fauxAssistantMessage("", { stopReason: "error" })],
    });
    expect(await main(["hi"], retry.io)).toBe(1);
    expect(existsSync(join(dir, ".lock"))).toBe(false);
  });
});
