export type CheckRunResult<T> =
  | { kind: "current"; value: T }
  | { kind: "failed"; error: unknown }
  | { kind: "stale" };

export class CheckRunOwner {
  private generation = 0;
  private active: AbortController | undefined;
  private disposed = false;

  async run<T>(
    work: (signal: AbortSignal) => Promise<T>,
    publish?: (value: T, signal: AbortSignal) => void | Promise<void>,
    publishFailure?: (error: unknown, signal: AbortSignal) => void | Promise<void>,
  ): Promise<CheckRunResult<T>> {
    if (this.disposed) {
      return { kind: "stale" };
    }

    const generation = ++this.generation;
    this.active?.abort();
    const controller = new AbortController();
    this.active = controller;

    try {
      const value = await work(controller.signal);
      if (!this.isCurrent(generation, controller)) {
        return { kind: "stale" };
      }
      await publish?.(value, controller.signal);
      return this.isCurrent(generation, controller)
        ? { kind: "current", value }
        : { kind: "stale" };
    } catch (error) {
      if (!this.isCurrent(generation, controller)) {
        return { kind: "stale" };
      }
      await publishFailure?.(error, controller.signal);
      return this.isCurrent(generation, controller) ? { kind: "failed", error } : { kind: "stale" };
    } finally {
      if (this.isCurrent(generation, controller)) {
        this.active = undefined;
      }
    }
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.generation += 1;
    this.active?.abort();
    this.active = undefined;
  }

  private isCurrent(generation: number, controller: AbortController): boolean {
    return !this.disposed && this.generation === generation && this.active === controller;
  }
}
