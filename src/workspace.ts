import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  mkdirSync,
  openSync,
  realpathSync,
} from "node:fs";
import {
  dirname,
  extname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";

/** Shared path boundary for draft, PDF and note tools. Not a sandbox against concurrent ancestor replacement. */
export function workspace(cwd: string) {
  const root = realpathSync(cwd);
  function path(
    input: string,
    extensions: readonly string[],
    createParents = false,
  ): string {
    const target = resolve(root, input);
    const local = relative(root, target);
    if (
      !local ||
      local === ".." ||
      local.startsWith(`..${sep}`) ||
      isAbsolute(local)
    ) {
      throw new Error("path must be inside the workspace");
    }
    if (!extensions.includes(extname(target).toLowerCase())) {
      throw new Error(`path must end in ${extensions.join(" or ")}`);
    }
    let parent = root;
    for (const part of relative(root, dirname(target))
      .split(sep)
      .filter(Boolean)) {
      parent = join(parent, part);
      if (createParents) {
        try {
          mkdirSync(parent);
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
        }
      }
      const stat = lstatSync(parent);
      if (stat.isSymbolicLink() || !stat.isDirectory()) {
        throw new Error(
          "parent must be a real workspace directory; symlinks are not allowed",
        );
      }
    }
    return target;
  }
  function withFile<T>(
    target: string,
    writable: boolean,
    action: (fd: number) => T,
  ): T {
    const stat = lstatSync(target);
    if (!stat.isFile() || stat.nlink !== 1) {
      throw new Error(
        "file must be regular with one link; symlinks and hard links are not allowed",
      );
    }
    const fd = openSync(
      target,
      (writable ? constants.O_RDWR : constants.O_RDONLY) |
        constants.O_NOFOLLOW |
        constants.O_NONBLOCK,
    );
    try {
      const opened = fstatSync(fd);
      if (
        !opened.isFile() ||
        opened.nlink !== 1 ||
        opened.dev !== stat.dev ||
        opened.ino !== stat.ino
      ) {
        throw new Error(
          "file changed while opening; retry after checking the workspace",
        );
      }
      return action(fd);
    } finally {
      closeSync(fd);
    }
  }
  return { path, withFile };
}
