#!/usr/bin/env bun
import { parseArgs } from "node:util";
import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import { createModels, type MutableModels } from "@earendil-works/pi-ai";
import { anthropicProvider } from "@earendil-works/pi-ai/providers/anthropic";
import { zaiProvider } from "@earendil-works/pi-ai/providers/zai";
import systemPrompt from "./prompt.md" with { type: "text" };

const PROVIDERS = {
  anthropic: {
    create: anthropicProvider,
    envVar: "ANTHROPIC_API_KEY",
    model: "claude-opus-5-5",
  },
  zai: { create: zaiProvider, envVar: "ZAI_API_KEY", model: "glm-5.2" },
} as const;

type ProviderId = keyof typeof PROVIDERS;

/** Process-facing dependencies, injectable so tests never touch the real environment or network. */
export interface Io {
  env: Record<string, string | undefined>;
  stdout: (text: string) => void;
  stderr: (text: string) => void;
  /** Replaces the model call. Tests use it to stream scripted events. */
  streamFn?: StreamFn;
}

export async function main(argv: string[], io: Io): Promise<number> {
  const { values, positionals } = parseArgs({
    args: argv,
    options: { json: { type: "boolean", default: false } },
    allowPositionals: true,
  });
  const prompt = positionals.join(" ").trim();
  if (!prompt) {
    io.stderr('usage: nahida [--json] "<prompt>"\n');
    return 2;
  }

  const providerId = io.env.NAHIDA_PROVIDER ?? "anthropic";
  if (!Object.hasOwn(PROVIDERS, providerId)) {
    io.stderr(
      `nahida: unknown NAHIDA_PROVIDER "${providerId}" (expected anthropic or zai)\n`,
    );
    return 2;
  }
  const provider = PROVIDERS[providerId as ProviderId];

  const models: MutableModels = createModels({
    authContext: {
      env: async (name) => io.env[name],
      fileExists: async () => false,
    },
  });
  models.setProvider(provider.create());

  // Credentials are checked locally, before any request leaves the process.
  if (!(await models.checkAuth(providerId))) {
    io.stderr(
      `nahida: missing credentials for ${providerId}; set ${provider.envVar}\n`,
    );
    return 1;
  }

  const modelId = io.env.NAHIDA_MODEL ?? provider.model;
  const model = models.getModel(providerId, modelId);
  if (!model) {
    io.stderr(`nahida: unknown model "${modelId}" for ${providerId}\n`);
    return 2;
  }

  const agent = new Agent({
    initialState: { systemPrompt, model },
    streamFn: io.streamFn ?? models.streamSimple.bind(models),
  });

  agent.subscribe((event) => {
    if (values.json) {
      io.stdout(`${JSON.stringify(event)}\n`);
    } else if (
      event.type === "message_update" &&
      event.assistantMessageEvent.type === "text_delta"
    ) {
      io.stdout(event.assistantMessageEvent.delta);
    }
  });

  await agent.prompt(prompt);
  if (!values.json) io.stdout("\n");

  if (agent.state.errorMessage) {
    io.stderr(`nahida: ${agent.state.errorMessage}\n`);
    return 1;
  }
  return 0;
}

if (import.meta.main) {
  process.exitCode = await main(Bun.argv.slice(2), {
    env: process.env,
    stdout: (text) => process.stdout.write(text),
    stderr: (text) => process.stderr.write(text),
  });
}
