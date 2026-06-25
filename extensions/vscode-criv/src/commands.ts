export const COMMAND_REFRESH_STATE_VIEW = "criv.refreshStateView";
export const COMMAND_OPEN_STATE_JSON = "criv.openStateJson";
export const COMMAND_OPEN_SOURCE_TARGET = "criv.openSourceTarget";
export const COMMAND_RUN_WATCH_ONCE = "criv.runWatchOnce";
export const COMMAND_RUN_CHECK = "criv.runCheck";
export const COMMAND_QUERY_UNDOCUMENTED_CODE = "criv.queryUndocumentedCode";

export const CRIV_COMMANDS = [
  COMMAND_REFRESH_STATE_VIEW,
  COMMAND_OPEN_STATE_JSON,
  COMMAND_OPEN_SOURCE_TARGET,
  COMMAND_RUN_WATCH_ONCE,
  COMMAND_RUN_CHECK,
  COMMAND_QUERY_UNDOCUMENTED_CODE,
] as const;
