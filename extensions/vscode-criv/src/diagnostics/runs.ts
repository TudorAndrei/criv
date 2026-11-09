/** Runs one check at a time: a new run aborts the previous one, and only the current run publishes. */
export class CheckRunOwner {
  private generation = 0;
  private active: AbortController | undefined;
  private disposed = false;

  async run<T>(
    work: (signal: AbortSignal) => Promise<T>,
    publish: (value: T) => void | Promise<void>,
    publishFailure: (error: unknown) => void | Promise<void>,
  ): Promise<void> {
    if (this.disposed) {
      return;
    }

    const generation = ++this.generation;
    this.active?.abort();
    const controller = new AbortController();
    this.active = controller;

    try {
      const value = await work(controller.signal);
      if (this.isCurrent(generation, controller)) {
        await publish(value);
      }
    } catch (error) {
      if (this.isCurrent(generation, controller)) {
        await publishFailure(error);
      }
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
