export interface DisposableRevision {
  dispose(): void;
}

export interface RevisionLoadAttempt {
  assertCurrent(): void;
}

export type RevisionLoadResult<Value> =
  | { kind: "committed"; value: Value }
  | { kind: "failed"; error: unknown }
  | { kind: "superseded" }
  | { kind: "closed" };

export class LoadedRevisionOwner<Revision extends DisposableRevision> {
  private active: Revision | undefined;
  private sequence = 0;
  private closed = false;

  get current(): Revision | undefined {
    return this.active;
  }

  async replace<Value>(
    load: (attempt: RevisionLoadAttempt) => Promise<Revision>,
    prepare: (revision: Revision) => Value,
  ): Promise<RevisionLoadResult<Value>> {
    if (this.closed) {
      return { kind: "closed" };
    }

    const sequence = ++this.sequence;
    const isCurrent = () => !this.closed && sequence === this.sequence;
    const attempt = this.attempt(isCurrent);
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

  dispose(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.sequence += 1;
    this.disposeActive();
  }

  private attempt(isCurrent: () => boolean): RevisionLoadAttempt {
    return {
      assertCurrent() {
        if (!isCurrent()) {
          throw new SupersededRevisionLoadError();
        }
      },
    };
  }

  private disposeActive(): void {
    const active = this.active;
    this.active = undefined;
    active?.dispose();
  }
}

class SupersededRevisionLoadError extends Error {
  constructor() {
    super("The loaded revision was superseded.");
    this.name = "SupersededRevisionLoadError";
  }
}
