import type { PackRegistry } from "../pack";
import { echoPack } from "./echo";
import { readingPack } from "./reading";
import { writingPack } from "./writing";

/** Fresh tools bind the host-selected workspace for this run. */
export function builtinPacks(cwd: string): PackRegistry {
  return {
    echo: echoPack,
    writing: writingPack(cwd),
    reading: readingPack(cwd),
  };
}
