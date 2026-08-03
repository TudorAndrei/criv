export const CRIV_LIKEC4_PROTOCOL_VERSION = 1 as const;
export const CRIV_LIKEC4_NODE_VERSION = "26.5.1" as const;
export const CRIV_LIKEC4_VERSION = "1.59.2" as const;

export interface CrivLikeC4Position {
  line: number;
  character: number;
}

export interface CrivLikeC4Range {
  start: CrivLikeC4Position;
  end: CrivLikeC4Position;
}

export interface CrivLikeC4Diagnostic {
  message: string;
  file: string;
  line: number | null;
  range: CrivLikeC4Range | null;
}

export interface CrivLikeC4BridgeResponse {
  protocolVersion: typeof CRIV_LIKEC4_PROTOCOL_VERSION;
  nodeVersion: string;
  likec4Version: typeof CRIV_LIKEC4_VERSION;
  revision: number;
  valid: boolean;
  diagnostics: CrivLikeC4Diagnostic[];
  model: unknown | null;
  bridgeError?: string;
}

export interface CrivLikeC4Model {
  protocolVersion: typeof CRIV_LIKEC4_PROTOCOL_VERSION;
  likec4Version: typeof CRIV_LIKEC4_VERSION;
  revision: number;
  model: unknown;
  views: { id: string; title: string; sourcePath?: string }[];
  sourceLinks: { element: string; target: string }[];
}
