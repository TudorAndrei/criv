import type {
  DisposableRevision,
  RevisionLoadAttempt,
  RevisionLoadResult,
} from "./revisionOwner.js";

export type GenerationRevisionResult<Value> = RevisionLoadResult<Value> | { kind: "stale" };

export class GenerationRevisionOwner<Revision extends DisposableRevision> {
  private active: Revision | undefined;
  private generation = -1;
  private sequence = 0;
  private closed = false;

  get current(): Revision | undefined {
    return this.active;
  }

  async replace<Value>(
    generation: number,
    load: (attempt: RevisionLoadAttempt) => Promise<Revision>,
    prepare: (revision: Revision) => Value,
  ): Promise<GenerationRevisionResult<Value>> {
    if (this.closed || generation < this.generation) {
      return { kind: "stale" };
    }
    this.generation = generation;
    const sequence = ++this.sequence;
    const isCurrent = () => !this.closed && sequence === this.sequence;
    const attempt: RevisionLoadAttempt = {
      assertCurrent() {
        if (!isCurrent()) {
          throw new SupersededGenerationRevisionError();
        }
      },
    };
    let candidate: Revision | undefined;
    try {
      candidate = await load(attempt);
      attempt.assertCurrent();
      const value = prepare(candidate);
      const previous = this.active;
      this.active = candidate;
      candidate = undefined;
      previous?.dispose();
      return { kind: "committed", value };
    } catch (error) {
      candidate?.dispose();
      if (!isCurrent()) {
        return { kind: "superseded" };
      }
      this.disposeActive();
      return { kind: "failed", error };
    }
  }

  clear(generation: number): boolean {
    if (this.closed || generation <= this.generation) {
      return false;
    }
    this.generation = generation;
    this.sequence += 1;
    this.disposeActive();
    return true;
  }

  invalidate(): void {
    if (this.closed) {
      return;
    }
    this.sequence += 1;
    this.disposeActive();
  }

  dispose(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.sequence += 1;
    this.disposeActive();
  }

  private disposeActive(): void {
    const active = this.active;
    this.active = undefined;
    active?.dispose();
  }
}

class SupersededGenerationRevisionError extends Error {
  constructor() {
    super("The generation revision was superseded.");
    this.name = "SupersededGenerationRevisionError";
  }
}
