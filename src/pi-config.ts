import { homedir } from "node:os";
import { join } from "node:path";
import { InMemoryModelsStore } from "@earendil-works/pi-ai";
import { ModelRuntime, SettingsManager } from "@earendil-works/pi-coding-agent";
import { BootError } from "./pack";

/** pi's agent dir: `PI_CODING_AGENT_DIR` when set, else `~/.pi/agent`. */
export function piAgentDir(env: Record<string, string | undefined>): string {
  const dir = env.PI_CODING_AGENT_DIR;
  if (!dir) return join(homedir(), ".pi", "agent");
  return dir.startsWith("~/") ? join(homedir(), dir.slice(2)) : dir;
}

/**
 * Providers, models and credentials come from pi's own loaders, so auth.json
 * locking and OAuth refresh behave as they do for pi itself. The model catalog
 * is kept in memory so nothing is written into the agent dir. Creation does no
 * network I/O and does not refresh OAuth tokens.
 */
export async function openPiConfig(agentDir: string) {
  const runtime = await ModelRuntime.create({
    authPath: join(agentDir, "auth.json"),
    modelsPath: join(agentDir, "models.json"),
    modelsStore: new InMemoryModelsStore(),
    refreshOnCreate: false,
  });
  // Project settings are ignored: the workspace is untrusted input for model choice.
  const settings = SettingsManager.create(process.cwd(), agentDir, {
    projectTrusted: false,
  });
  return { runtime, settings };
}

/** The profile's model wins; otherwise settings.json's defaultProvider and defaultModel. */
export function modelRef(
  profileModel: string | undefined,
  settings: SettingsManager,
): string {
  if (profileModel) return profileModel;
  const provider = settings.getDefaultProvider();
  const id = settings.getDefaultModel();
  if (provider && id) return `${provider}/${id}`;
  throw new BootError(
    "no model: set model in the profile, or defaultProvider and defaultModel in settings.json",
  );
}

/** Splits at the first slash: the provider id has none, the model id may. */
export function splitModelRef(ref: string): { provider: string; id: string } {
  const slash = ref.indexOf("/");
  return { provider: ref.slice(0, slash), id: ref.slice(slash + 1) };
}
