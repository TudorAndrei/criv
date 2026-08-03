import type { CommandResult } from "./commandRunner";

export const CHECK_MAX_OUTPUT_BYTES = 16 * 1024 * 1024;

export function completeCheckStdout(result: CommandResult): string | undefined {
  return result.stdoutTruncated ? undefined : result.stdout;
}
