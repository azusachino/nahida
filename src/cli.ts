#!/usr/bin/env bun
import { parseArgs } from "node:util";
import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import type { Api, Model } from "@earendil-works/pi-ai";
import type { ModelRuntime } from "@earendil-works/pi-coding-agent";
import {
  activatePacks,
  BootError,
  collectTools,
  type Pack,
  type PackRegistry,
  type PackTool,
  selectPacks,
} from "./pack";
import { builtinPacks } from "./packs";
import { modelRef, openPiConfig, piAgentDir, splitModelRef } from "./pi-config";
import { loadProfile } from "./profile";
import systemPrompt from "./prompt.md" with { type: "text" };

/** Process-facing dependencies, injectable so tests never touch the real environment or network. */
export interface Io {
  env: Record<string, string | undefined>;
  stdout: (text: string) => void;
  stderr: (text: string) => void;
  /** Replaces the model call. Tests use it to stream scripted events. */
  streamFn?: StreamFn;
  /** Replaces the built-in packs. Tests use it to add packs that record activation. */
  packs?: PackRegistry;
}

const USAGE =
  'usage: nahida [--json] [--profile <name>] [--describe] "<prompt>"\n';

export async function main(argv: string[], io: Io): Promise<number> {
  const { values, positionals } = parseArgs({
    args: argv,
    options: {
      json: { type: "boolean", default: false },
      describe: { type: "boolean", default: false },
      profile: { type: "string", default: "default" },
    },
    allowPositionals: true,
  });
  const prompt = positionals.join(" ").trim();
  if (!values.describe && !prompt) {
    io.stderr(USAGE);
    return 2;
  }

  try {
    const registry = io.packs ?? builtinPacks;
    const profile = await loadProfile(values.profile, io.env, registry);
    const packs = selectPacks(profile.packs, registry, profile.name);
    const tools = collectTools(packs);

    const agentDir = piAgentDir(io.env);
    const { runtime, settings } = await openPiConfig(agentDir);
    const { provider, id } = splitModelRef(modelRef(profile.model, settings));
    const model = runtime.getModel(provider, id);
    if (!model) {
      io.stderr(`nahida: unknown model "${id}" for ${provider}\n`);
      return 2;
    }

    // Credentials are checked locally, before any request leaves the process.
    const auth = await runtime.checkAuth(provider);

    if (values.describe) {
      const credentials = auth
        ? `${auth.type}${auth.source ? ` (${auth.source})` : ""}`
        : "none";
      io.stdout(
        [
          `profile: ${profile.name}`,
          `model: ${provider}/${id}`,
          `credentials: ${credentials}`,
          `packs: ${packs.map((pack) => pack.id).join(", ") || "none"}`,
          `tools: ${tools.map((tool) => tool.name).join(", ") || "none"}`,
          `prompt: ${promptSources(packs)}`,
          `approval: ${profile.approval}`,
          `agent dir: ${agentDir}`,
          "",
        ].join("\n"),
      );
      return 0;
    }

    if (!auth) {
      io.stderr(
        `nahida: no credentials for provider ${provider}; run /login in pi, or add a key for it to ${agentDir}/auth.json\n`,
      );
      return 1;
    }

    return await run({
      io,
      prompt,
      json: values.json,
      model,
      packs,
      tools,
      runtime,
    });
  } catch (error) {
    if (error instanceof BootError) {
      io.stderr(`nahida: ${error.message}\n`);
      return 2;
    }
    throw error;
  }
}

function promptSources(packs: readonly Pack[]): string {
  const sources = packs
    .filter((pack) => pack.prompt)
    .map((pack) => `${pack.id} (${pack.prompt?.file})`);
  return sources.join(", ") || "none";
}

interface RunOptions {
  io: Io;
  prompt: string;
  json: boolean;
  model: Model<Api>;
  packs: Pack[];
  tools: PackTool[];
  runtime: ModelRuntime;
}

async function run({
  io,
  prompt,
  json,
  model,
  packs,
  tools,
  runtime,
}: RunOptions): Promise<number> {
  // SIGINT aborts the run. Disposal happens in the finally below, so it also covers the signal path.
  let interrupted = false;
  let agent: Agent | undefined;
  const onSigint = () => {
    interrupted = true;
    agent?.abort();
  };
  process.on("SIGINT", onSigint);

  let active: { dispose: () => Promise<void> } | undefined;
  try {
    active = await activatePacks(packs, { cwd: process.cwd() });
    const prompts = [
      systemPrompt,
      ...packs.map((pack) => pack.prompt?.text ?? ""),
    ];
    agent = new Agent({
      initialState: {
        systemPrompt: prompts.filter((text) => text.length > 0).join("\n\n"),
        model,
        tools,
      },
      streamFn: io.streamFn ?? runtime.streamSimple.bind(runtime),
    });

    agent.subscribe((event) => {
      if (json) {
        io.stdout(`${JSON.stringify(event)}\n`);
      } else if (
        event.type === "message_update" &&
        event.assistantMessageEvent.type === "text_delta"
      ) {
        io.stdout(event.assistantMessageEvent.delta);
      }
    });

    if (!interrupted) await agent.prompt(prompt);
  } finally {
    process.off("SIGINT", onSigint);
    await active?.dispose().catch((error: unknown) => {
      io.stderr(
        `nahida: ${error instanceof Error ? error.message : String(error)}\n`,
      );
    });
  }
  if (!json) io.stdout("\n");

  if (interrupted) return 130;
  if (agent?.state.errorMessage) {
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
