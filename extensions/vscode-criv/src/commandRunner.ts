import { spawn } from "node:child_process";

export interface CommandResult {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
  cancelled: boolean;
}

export interface CommandRunOptions {
  cwd: string;
  signal?: AbortSignal;
}

export function runProcess(
  command: string,
  args: readonly string[],
  options: CommandRunOptions,
): Promise<CommandResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let cancelled = false;

    const cancel = () => {
      cancelled = true;
      child.kill();
    };

    options.signal?.addEventListener("abort", cancel, { once: true });
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.on("error", (error) => {
      options.signal?.removeEventListener("abort", cancel);
      reject(error);
    });
    child.on("close", (code, signal) => {
      options.signal?.removeEventListener("abort", cancel);
      resolve({
        code,
        signal,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
        cancelled,
      });
    });
  });
}
