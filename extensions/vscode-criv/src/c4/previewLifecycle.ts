import {
  GenerationRevisionOwner,
  type DisposableRevision,
  type RevisionLoadAttempt,
} from "@criv/editor-state";

export interface PreviewGenerationStatus {
  generation: number;
  kind: string;
}

export class C4PreviewLifecycle<Revision extends DisposableRevision> {
  private readonly revisions = new GenerationRevisionOwner<Revision>();
  private loadingGeneration = -1;
  private closed = false;

  get current(): Revision | undefined {
    return this.revisions.current;
  }

  async publish<Status extends PreviewGenerationStatus>(
    status: Status,
    load: ((attempt: RevisionLoadAttempt) => Promise<Revision>) | undefined,
    render: (revision: Revision) => void,
    showStatus: (status: Status) => void,
    showRenderError?: (error: unknown) => void,
  ): Promise<void> {
    if (this.closed) {
      return;
    }
    if (status.kind === "loading") {
      if (!this.revisions.current && status.generation > this.loadingGeneration) {
        this.loadingGeneration = status.generation;
        showStatus(status);
      }
      return;
    }
    if (status.kind !== "ready") {
      if (this.revisions.clear(status.generation)) {
        showStatus(status);
      }
      return;
    }
    if (!load) {
      return;
    }
    const result = await this.revisions.replace(status.generation, load, (candidate) => {
      render(candidate);
    });
    if (result.kind === "failed") {
      showRenderError?.(result.error);
    }
  }

  dispose(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.revisions.dispose();
  }
}
