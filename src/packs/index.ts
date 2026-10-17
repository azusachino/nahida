import type { PackRegistry } from "../pack";
import { echoPack } from "./echo";

/** Packs shipped with nahida, keyed by pack id. */
export const builtinPacks: PackRegistry = {
  echo: echoPack,
};
