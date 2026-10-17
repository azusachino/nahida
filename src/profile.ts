import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { BootError, type PackRegistry } from "./pack";

export interface Profile {
  name: string;
  /** Pack ids in load order. */
  packs: string[];
  /** `provider/id`. When absent, settings.json's defaults apply. */
  model?: string;
  /** Approval policy. Recorded and shown by --describe; no pack enforces it yet. */
  approval: string;
}

const KEYS = new Set(["packs", "model", "approval"]);
const NAME = /^[a-z0-9][a-z0-9-]*$/;

/** `NAHIDA_CONFIG_DIR` replaces `~/.config/nahida`; profiles live in its `profiles/`. */
export function profilesDir(env: Record<string, string | undefined>): string {
  const root = env.NAHIDA_CONFIG_DIR ?? join(homedir(), ".config", "nahida");
  return join(root, "profiles");
}

/**
 * Reads `<profiles>/<name>.toml`. With no file, `default` is built in and loads
 * every pack in the registry; any other missing name is a boot error.
 */
export async function loadProfile(
  name: string,
  env: Record<string, string | undefined>,
  registry: PackRegistry,
): Promise<Profile> {
  if (!NAME.test(name)) {
    throw new BootError(`invalid profile name "${name}"`);
  }
  const file = join(profilesDir(env), `${name}.toml`);
  if (!existsSync(file)) {
    if (name === "default") {
      return { name, packs: Object.keys(registry), approval: "ask" };
    }
    throw new BootError(`profile "${name}" not found at ${file}`);
  }
  return parseProfile(name, await Bun.file(file).text(), file);
}

export function parseProfile(
  name: string,
  text: string,
  file: string,
): Profile {
  let raw: Record<string, unknown>;
  try {
    raw = Bun.TOML.parse(text) as Record<string, unknown>;
  } catch (error) {
    throw new BootError(`${file}: ${(error as Error).message}`);
  }
  const unknown = Object.keys(raw).filter((key) => !KEYS.has(key));
  if (unknown.length > 0) {
    throw new BootError(`${file}: unknown key "${unknown[0]}"`);
  }
  const { packs, model, approval } = raw;
  if (
    !Array.isArray(packs) ||
    !packs.every((id) => typeof id === "string" && id.length > 0)
  ) {
    throw new BootError(`${file}: "packs" must be a list of pack ids`);
  }
  if (
    model !== undefined &&
    (typeof model !== "string" || !model.includes("/"))
  ) {
    throw new BootError(`${file}: "model" must be "<provider>/<id>"`);
  }
  if (approval !== undefined && typeof approval !== "string") {
    throw new BootError(`${file}: "approval" must be a string`);
  }
  return {
    name,
    packs: packs as string[],
    model: model as string | undefined,
    approval: (approval as string | undefined) ?? "ask",
  };
}
