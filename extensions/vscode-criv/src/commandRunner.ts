import { spawn } from "node:child_process";
import { StringDecoder } from "node:string_decoder";

const DEFAULT_MAX_OUTPUT_BYTES = 1024 * 1024;
const DEFAULT_FORCE_KILL_AFTER_MS = 2_000;

export interface CommandResult {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
  stdoutTruncated: boolean;
  stderrTruncated: boolean;
  cancelled: boolean;
}

export interface CommandRunOptions {
  cwd: string;
  signal?: AbortSignal;
  maxOutputBytes?: number;
  forceKillAfterMs?: number;
}

export function runProcess(
  command: string,
  args: readonly string[],
  options: CommandRunOptions,
): Promise<CommandResult> {
  return new Promise((resolve, reject) => {
    const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
    const forceKillAfterMs = options.forceKillAfterMs ?? DEFAULT_FORCE_KILL_AFTER_MS;
    const child = spawn(command, args, {
      cwd: options.cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    const stdoutState = { bytes: 0, truncated: false };
    const stderrState = { bytes: 0, truncated: false };
    let cancelled = false;
    let settled = false;
    let forceKillTimer: NodeJS.Timeout | undefined;

    const cancel = () => {
      cancelled = true;
      child.kill("SIGTERM");
      forceKillTimer ??= setTimeout(() => {
        if (!settled) {
          child.kill("SIGKILL");
        }
      }, forceKillAfterMs);
    };

    options.signal?.addEventListener("abort", cancel, { once: true });
    child.stdout.on("data", (chunk: Buffer) => {
      captureOutput(stdout, stdoutState, chunk, maxOutputBytes);
    });
    child.stderr.on("data", (chunk: Buffer) => {
      captureOutput(stderr, stderrState, chunk, maxOutputBytes);
    });
    child.on("error", (error) => {
      settled = true;
      cleanup(child.stdout, child.stderr, cancel, forceKillTimer, options.signal);
      reject(error);
    });
    child.on("close", (code, signal) => {
      settled = true;
      cleanup(child.stdout, child.stderr, cancel, forceKillTimer, options.signal);
      resolve({
        code,
        signal,
        stdout: capturedText(stdout, stdoutState),
        stderr: capturedText(stderr, stderrState),
        stdoutTruncated: stdoutState.truncated,
        stderrTruncated: stderrState.truncated,
        cancelled,
      });
    });
  });
}

function captureOutput(
  buffers: Buffer[],
  state: { bytes: number; truncated: boolean },
  chunk: Buffer,
  maxOutputBytes: number,
): void {
  if (state.bytes >= maxOutputBytes) {
    state.truncated = true;
    return;
  }
  const remaining = maxOutputBytes - state.bytes;
  if (chunk.length <= remaining) {
    buffers.push(chunk);
    state.bytes += chunk.length;
    return;
  }
  buffers.push(chunk.subarray(0, remaining));
  state.bytes = maxOutputBytes;
  state.truncated = true;
}

function capturedText(buffers: Buffer[], state: { bytes: number; truncated: boolean }): string {
  const decoder = new StringDecoder("utf8");
  const captured = Buffer.concat(buffers, state.bytes);
  return state.truncated ? decoder.write(captured) : decoder.end(captured);
}

function cleanup(
  stdout: NodeJS.ReadableStream | null,
  stderr: NodeJS.ReadableStream | null,
  cancel: () => void,
  forceKillTimer: NodeJS.Timeout | undefined,
  signal: AbortSignal | undefined,
): void {
  signal?.removeEventListener("abort", cancel);
  stdout?.removeAllListeners("data");
  stderr?.removeAllListeners("data");
  if (forceKillTimer) {
    clearTimeout(forceKillTimer);
  }
}
