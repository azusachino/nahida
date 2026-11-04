#!/usr/bin/env bun
import { realpathSync } from "node:fs";
import { parseArgs } from "node:util";
import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import { type Api, type Model, toToolDeclaration } from "@earendil-works/pi-ai";
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
import { openSession, sessionDir } from "./session";

/** Process-facing dependencies, injectable so tests never touch the real environment or network. */
export interface Io {
  env: Record<string, string | undefined>;
  cwd?: string;
  stdout: (text: string) => void;
  stderr: (text: string) => void;
  /** Replaces the model call. Tests use it to stream scripted events. */
  streamFn?: StreamFn;
  /** Replaces the built-in packs. Tests use it to add packs that record activation. */
  packs?: PackRegistry;
}

const USAGE =
  'usage: nahida [--json] [--profile <name>] [--describe] [--continue] ["<prompt>"]\n';

export async function main(argv: string[], io: Io): Promise<number> {
  const { values, positionals } = parseArgs({
    args: argv,
    options: {
      json: { type: "boolean", default: false },
      describe: { type: "boolean", default: false },
      continue: { type: "boolean", default: false },
      profile: { type: "string", default: "default" },
    },
    allowPositionals: true,
  });
  const prompt = positionals.join(" ").trim();
  if (!values.describe && !values.continue && !prompt) {
    io.stderr(USAGE);
    return 2;
  }

  try {
    const registry = io.packs ?? builtinPacks;
    const profile = await loadProfile(values.profile, io.env, registry);
    const packs = selectPacks(profile.packs, registry, profile.name);
    const tools = collectTools(packs);
    const cwd = realpathSync(io.cwd ?? process.cwd());
    const sessions = sessionDir(io.env, cwd);

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
          `session dir: ${sessions}`,
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
      cwd,
      sessions,
      resume: values.continue,
      approval: profile.approval,
    });
  } catch (error) {
    if (error instanceof BootError) {
      io.stderr(`nahida: ${error.message}\n`);
      return 2;
    }
    io.stderr(
      `nahida: ${error instanceof Error ? error.message : String(error)}\n`,
    );
    return 1;
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
  cwd: string;
  sessions: string;
  resume: boolean;
  approval: string;
}

async function run({
  io,
  prompt,
  json,
  model,
  packs,
  tools,
  runtime,
  cwd,
  sessions,
  resume,
  approval,
}: RunOptions): Promise<number> {
  const prompts = [
    systemPrompt,
    ...packs.map((pack) => pack.prompt?.text ?? ""),
  ];
  const instructions = prompts.filter((text) => text.length > 0).join("\n\n");
  const session = openSession({
    dir: sessions,
    cwd,
    resume,
    provider: model.provider,
    modelId: model.id,
    signature: JSON.stringify({
      packs: packs.map((pack) => pack.id),
      instructions,
      tools: tools.map(toToolDeclaration),
      approval,
    }),
  });
  const { manager } = session;
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
    if (session.fresh) {
      manager.appendMessage({
        role: "system",
        content: instructions,
        toolsAdded: tools.map(toToolDeclaration),
        timestamp: Date.now(),
      });
    }
    if (!prompt) {
      const tail = manager.buildSessionContext().messages.at(-1);
      if (tail?.role !== "user" && tail?.role !== "toolResult") {
        throw new BootError(
          'the session has no unfinished input; use --continue "<prompt>"',
        );
      }
    }
    const pendingContext: string[] = [];
    active = await activatePacks(packs, {
      cwd,
      addContext: (text) => {
        pendingContext.push(text);
      },
    });
    let failed = false;
    agent = new Agent({
      initialState: {
        messages: manager.buildSessionContext().messages,
        model,
        tools,
      },
      sessionId: manager.getSessionId(),
      streamFn: io.streamFn ?? runtime.streamSimple.bind(runtime),
      prepareRequest: ({ context }) => {
        // A tool may add context while its result is still pending. Commit it
        // only at the request boundary, after all tool results are recorded.
        for (const text of pendingContext.splice(0)) {
          manager.appendMessage({
            role: "user",
            content: text,
            timestamp: Date.now(),
          });
        }
        return {
          context: {
            ...context,
            messages: manager.buildSessionContext().messages,
          },
        };
      },
      finishTurn: () => (interrupted || failed ? { action: "end" } : undefined),
    });

    agent.subscribe((event) => {
      if (event.type === "message_end") {
        const message = event.message;
        if (
          message.role === "assistant" &&
          message.stopReason !== "stop" &&
          message.stopReason !== "toolUse"
        ) {
          failed = true;
          manager.appendCustomEntry("nahida.failure", {
            stopReason: message.stopReason,
            error: message.errorMessage,
          });
          // message_end is an awaited pi barrier before tool execution. An
          // unsupported/partial terminal result must stop the loop here.
          if (
            message.stopReason !== "error" &&
            message.stopReason !== "aborted"
          ) {
            throw new Error(
              `incomplete model response (${message.stopReason})`,
            );
          }
        } else if (
          message.role === "system" ||
          message.role === "user" ||
          message.role === "assistant" ||
          message.role === "toolResult"
        ) {
          manager.appendMessage(message);
        }
      }
      if (json) {
        io.stdout(`${JSON.stringify(event)}\n`);
      } else if (
        event.type === "message_update" &&
        event.assistantMessageEvent.type === "text_delta"
      ) {
        io.stdout(event.assistantMessageEvent.delta);
      }
    });

    if (!interrupted) {
      if (prompt) await agent.prompt(prompt);
      else await agent.continue();
    }
    if (failed && !interrupted) {
      io.stderr(
        `nahida: ${agent.state.errorMessage ?? "model response failed"}\n`,
      );
      return 1;
    }
  } finally {
    process.off("SIGINT", onSigint);
    try {
      await active?.dispose().catch((error: unknown) => {
        io.stderr(
          `nahida: ${error instanceof Error ? error.message : String(error)}\n`,
        );
      });
    } finally {
      session.release();
    }
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
