import type { CrivState, SourceIndexEntry, SourceResolver } from "./core";
import type { CrivSelectorSuggestion, CrivStateSummary } from "./wasm";

export type ObsidianStateStatus =
  | { generation: number; kind: "loading" }
  | { generation: number; kind: "ready"; state: CrivState }
  | { generation: number; kind: "missing"; message: string }
  | { generation: number; kind: "invalid"; message: string }
  | { generation: number; kind: "unavailable"; message: string };

export interface DisposableSubscription {
  dispose(): void;
}

export interface StatePort {
  currentStateStatus(): ObsidianStateStatus;
  onStateStatusChange(listener: (status: ObsidianStateStatus) => void): DisposableSubscription;
  getState(): Promise<CrivState | null>;
  stateStatus(): string;
  cachedState(): CrivState | null;
  cachedSourceResolver(): SourceResolver;
  suggestSourceSelectors(query: string, limit: number): CrivSelectorSuggestion[];
  recordWasmFailure(error: unknown): void;
  sourceEntries(): Promise<SourceIndexEntry[]>;
  patternIds(): Promise<string[]>;
  readState(): Promise<CrivStateSummary | null>;
}

export interface C4ViewPort {
  currentStateStatus(): ObsidianStateStatus;
  onStateStatusChange(listener: (status: ObsidianStateStatus) => void): DisposableSubscription;
  openValidatedSource(target: string): void;
}
