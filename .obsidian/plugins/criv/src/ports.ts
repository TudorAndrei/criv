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
  /** The loaded projections, loading State first when nothing has loaded yet. */
  load(): Promise<CrivInitialProjections | null>;
  current(): CrivInitialProjections | null;
  statusMessage(): string;
  sourceResolver(): SourceResolver;
  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[];
}
