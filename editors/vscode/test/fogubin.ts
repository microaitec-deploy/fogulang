import { existsSync } from "node:fs";
import { join } from "node:path";

/** The fogu binary used by the tests: $FOGU_BIN, else the repository build, else PATH. */
export const FOGU = process.env.FOGU_BIN ?? (() => {
  const local = join(import.meta.dir, "../../../target/release/fogu");
  return existsSync(local) ? local : "fogu";
})();
