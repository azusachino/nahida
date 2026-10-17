import { describe, expect, test } from "bun:test";
import { fauxAssistantMessage, fauxProvider } from "@earendil-works/pi-ai";
import { type Io, main } from "./cli";

/** Captures what the CLI writes and wires a scripted faux model in place of the network. */
function harness(
  env: Record<string, string | undefined>,
  responses: Parameters<
    ReturnType<typeof fauxProvider>["setResponses"]
  >[0] = [],
) {
  const faux = fauxProvider();
  faux.setResponses(responses);
  const stdout: string[] = [];
  const stderr: string[] = [];
  const io: Io = {
    env,
    stdout: (text) => stdout.push(text),
    stderr: (text) => stderr.push(text),
    streamFn: (model, context, options) =>
      faux.provider.streamSimple(model, context, options),
  };
  return { io, faux, stdout, stderr };
}

describe("missing credentials", () => {
  test("exits 1, names the missing variable, and makes no model call", async () => {
    const { io, faux, stderr } = harness({});
    const code = await main(["hello"], io);

    expect(code).toBe(1);
    expect(stderr.join("")).toContain("ANTHROPIC_API_KEY");
    expect(faux.state.callCount).toBe(0);
  });

  test("names the zai variable when that provider is selected", async () => {
    const { io, stderr } = harness({ NAHIDA_PROVIDER: "zai" });
    const code = await main(["hello"], io);

    expect(code).toBe(1);
    expect(stderr.join("")).toContain("ZAI_API_KEY");
  });
});

describe("--json", () => {
  test("emits one JSON object per line, each parseable", async () => {
    const { io, stdout } = harness({ ANTHROPIC_API_KEY: "test-key" }, [
      fauxAssistantMessage("Hello there."),
    ]);
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
    const { io, stdout } = harness({ ANTHROPIC_API_KEY: "test-key" }, [
      fauxAssistantMessage("Hello there."),
    ]);
    const code = await main(["say hi"], io);

    expect(code).toBe(0);
    expect(stdout.join("")).toContain("Hello there.");
  });

  test("a model error exits 1 and reports the message on stderr", async () => {
    const { io, stderr } = harness({ ANTHROPIC_API_KEY: "test-key" }, [
      fauxAssistantMessage("", {
        stopReason: "error",
        errorMessage: "upstream exploded",
      }),
    ]);
    const code = await main(["say hi"], io);

    expect(code).toBe(1);
    expect(stderr.join("")).toContain("upstream exploded");
  });
});
