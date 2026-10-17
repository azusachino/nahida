import type { AgentTool } from "@earendil-works/pi-agent-core";

/**
 * Pi types each tool with its own parameter schema, so a pack holds tools of
 * any schema. Each tool still declares its own parameters.
 */
// biome-ignore lint/suspicious/noExplicitAny: a pack holds tools with different parameter schemas
export type PackTool = AgentTool<any>;

/** A boot-time failure: bad profile, unknown pack, or a tool name two packs both register. */
export class BootError extends Error {}

export interface PackContext {
  cwd: string;
}

/** Undoes whatever a pack set up in `activate`. */
export type Disposer = () => void | Promise<void>;

/**
 * A pack is the unit a profile composes. Only the fields the first packs need:
 * tools for the model, a prompt fragment, and an optional activation hook.
 */
export interface Pack {
  id: string;
  tools?: PackTool[];
  /** `file` names the markdown source for `--describe`; `text` joins the system prompt. */
  prompt?: { file: string; text: string };
  activate?: (ctx: PackContext) => Disposer | Promise<Disposer>;
}

export type PackRegistry = Record<string, Pack>;

/** Looks up pack ids in order. An unknown id is a boot error. */
export function selectPacks(
  ids: readonly string[],
  registry: PackRegistry,
  profileName: string,
): Pack[] {
  return ids.map((id) => {
    const pack = Object.hasOwn(registry, id) ? registry[id] : undefined;
    if (!pack) {
      throw new BootError(`unknown pack "${id}" in profile "${profileName}"`);
    }
    return pack;
  });
}

/** All tools in pack order. Two packs registering the same name is a boot error. */
export function collectTools(packs: readonly Pack[]): PackTool[] {
  const owners = new Map<string, string>();
  const tools: PackTool[] = [];
  for (const pack of packs) {
    for (const tool of pack.tools ?? []) {
      const owner = owners.get(tool.name);
      if (owner !== undefined) {
        throw new BootError(
          `tool "${tool.name}" is registered by both pack "${owner}" and pack "${pack.id}"`,
        );
      }
      owners.set(tool.name, pack.id);
      tools.push(tool);
    }
  }
  return tools;
}

/** Activates packs in order. The returned `dispose` runs disposers in reverse, once. */
export async function activatePacks(
  packs: readonly Pack[],
  ctx: PackContext,
): Promise<{ dispose: () => Promise<void> }> {
  const disposers: Disposer[] = [];
  const dispose = async () => {
    const errors: unknown[] = [];
    for (const disposer of disposers.splice(0).reverse()) {
      try {
        await disposer();
      } catch (error) {
        errors.push(error);
      }
    }
    if (errors.length > 0) {
      throw new AggregateError(errors, "pack disposal failed");
    }
  };
  try {
    for (const pack of packs) {
      if (pack.activate) disposers.push(await pack.activate(ctx));
    }
  } catch (error) {
    await dispose().catch(() => {});
    throw error;
  }
  return { dispose };
}
