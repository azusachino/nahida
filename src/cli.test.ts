import { afterEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import {
  type Api,
  fauxAssistantMessage,
  fauxProvider,
  type Model,
} from "@earendil-works/pi-ai";
import { type Io, main } from "./cli";
import type { Pack, PackRegistry, PackTool } from "./pack";
import { echoPack, echoTool } from "./packs/echo";
import { piAgentDir } from "./pi-config";

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
    stdout: (text) => stdout.push(text),
    stderr: (text) => stderr.push(text),
    packs: options.packs,
    streamFn: (model, context, opts) => {
      calls.push({ model, context });
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
