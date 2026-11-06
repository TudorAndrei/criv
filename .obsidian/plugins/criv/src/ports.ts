import type {
  CrivInitialProjections,
  CrivSelectorSuggestion,
  CrivSourceEntry,
  CrivSourceTargetLookupResult,
} from "@criv/editor-state";

export interface SourceResolver {
  lookupSourceTarget(target: string): CrivSourceTargetLookupResult;
  sourceEntry(path: string): CrivSourceEntry | undefined;
}

/** The State queries that source features receive from the composition module. */
export interface StatePort {
  /** The settled projections: waits for the first or current load and never reloads. */
  load(): Promise<CrivInitialProjections | null>;
  current(): CrivInitialProjections | null;
  statusMessage(): string;
  sourceResolver(): SourceResolver;
  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[];
}
