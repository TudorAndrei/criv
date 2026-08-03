use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

const PROTOCOL_VERSION: u32 = 1;
const NODE_VERSION: &str = "26.5.1";
const LIKEC4_VERSION: &str = "1.59.2";

const BRIDGE_SOURCE: &str = include_str!("../assets/likec4-bridge.mjs");
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_BRIDGE_OUTPUT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LikeC4DiagnosticKind {
    Model,
    Runtime,
    Protocol,
}

impl LikeC4DiagnosticKind {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Model => "invalid-likec4",
            Self::Runtime => "missing-likec4-runtime",
            Self::Protocol => "invalid-likec4-protocol",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LikeC4Diagnostic {
    pub(crate) kind: LikeC4DiagnosticKind,
    pub(crate) path: String,
    pub(crate) line: Option<usize>,
    pub(crate) message: String,
}

#[derive(Debug, Default)]
pub(crate) struct LikeC4Workspace {
    pub(crate) path: String,
    pub(crate) version: Option<String>,
    pub(crate) diagnostics: Vec<LikeC4Diagnostic>,
    pub(crate) model: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeResponse {
    protocol_version: u32,
    node_version: String,
    likec4_version: String,
    revision: u64,
    valid: bool,
    #[serde(default)]
    diagnostics: Vec<BridgeDiagnostic>,
    model: Option<serde_json::Value>,
    bridge_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeDiagnostic {
    message: String,
    file: String,
    line: Option<usize>,
}

pub(crate) fn load(root: &Path, docs_path: &Path, sources: &[PathBuf]) -> LikeC4Workspace {
    if sources.is_empty() {
        return LikeC4Workspace::default();
    }

    let workspace_path = docs_path.join("architecture");
    let workspace_arg = workspace_path.to_string_lossy().to_string();
    let workspace = relative_path(root, &workspace_path);
    let source_paths = sources
        .iter()
        .map(|path| relative_path(root, path))
        .collect::<Vec<_>>();
    let mut result = LikeC4Workspace {
        path: workspace.clone(),
        ..LikeC4Workspace::default()
    };

    for path in &source_paths {
        if !path.starts_with(&format!("{workspace}/")) {
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Model,
                path: path.clone(),
                line: None,
                message: format!("LikeC4 source must be inside `{workspace}`"),
            });
        }
    }
    if !result.diagnostics.is_empty() {
        return result;
    }

    let mut child = match Command::new("node")
        .args([
            "--input-type=module",
            "--eval",
            BRIDGE_SOURCE,
            &workspace_arg,
            "0",
        ])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Runtime,
                path: workspace,
                line: None,
                message: format!(
                    "LikeC4 source requires Node.js 26.5.1 and local likec4 1.59.2: {error}"
                ),
            });
            return result;
        }
    };

    let stdout = child.stdout.take().expect("LikeC4 stdout is piped");
    let stderr = child.stderr.take().expect("LikeC4 stderr is piped");
    let stdout_reader = thread::spawn(move || read_capped(stdout));
    let stderr_reader = thread::spawn(move || read_capped(stderr));

    use wait_timeout::ChildExt;
    match child.wait_timeout(BRIDGE_TIMEOUT) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Runtime,
                path: workspace,
                line: None,
                message: "LikeC4 bridge exceeded the 60 second process limit".into(),
            });
            return result;
        }
        Err(error) => {
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Runtime,
                path: workspace,
                line: None,
                message: format!("failed to wait for LikeC4 bridge: {error}"),
            });
            return result;
        }
    }
    let stdout = match stdout_reader.join() {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Runtime,
                path: workspace,
                line: None,
                message: format!("failed to read LikeC4 bridge stdout: {error}"),
            });
            return result;
        }
        Err(_) => return bridge_reader_panic(result, workspace, "stdout"),
    };
    let stderr = match stderr_reader.join() {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Runtime,
                path: workspace,
                line: None,
                message: format!("failed to read LikeC4 bridge stderr: {error}"),
            });
            return result;
        }
        Err(_) => return bridge_reader_panic(result, workspace, "stderr"),
    };
    if stdout.len() > MAX_BRIDGE_OUTPUT || stderr.len() > MAX_BRIDGE_OUTPUT {
        result.diagnostics.push(LikeC4Diagnostic {
            kind: LikeC4DiagnosticKind::Protocol,
            path: workspace,
            line: None,
            message: "LikeC4 bridge output exceeded the 16 MiB limit".into(),
        });
        return result;
    }

    let response = match serde_json::from_slice::<BridgeResponse>(&stdout) {
        Ok(response) => response,
        Err(error) => {
            let stderr = String::from_utf8_lossy(&stderr);
            result.diagnostics.push(LikeC4Diagnostic {
                kind: LikeC4DiagnosticKind::Protocol,
                path: workspace,
                line: None,
                message: format!(
                    "LikeC4 bridge returned invalid JSON: {error}; stderr: {}",
                    stderr.trim()
                ),
            });
            return result;
        }
    };

    result.version = Some(response.likec4_version.clone());
    if response.protocol_version != PROTOCOL_VERSION
        || response.node_version != NODE_VERSION
        || response.likec4_version != LIKEC4_VERSION
        || response.revision != 0
    {
        result.diagnostics.push(LikeC4Diagnostic {
            kind: LikeC4DiagnosticKind::Protocol,
            path: workspace,
            line: None,
            message: format!(
                "LikeC4 bridge version mismatch: expected protocol {PROTOCOL_VERSION}, Node.js {NODE_VERSION}, and LikeC4 {LIKEC4_VERSION}; got protocol {}, Node.js {}, and LikeC4 {}",
                response.protocol_version, response.node_version, response.likec4_version
            ),
        });
        return result;
    }
    if let Some(error) = response.bridge_error {
        result.diagnostics.push(LikeC4Diagnostic {
            kind: LikeC4DiagnosticKind::Runtime,
            path: workspace,
            line: None,
            message: format!("LikeC4 bridge failed: {error}"),
        });
        return result;
    }

    result
        .diagnostics
        .extend(
            response
                .diagnostics
                .into_iter()
                .map(|diagnostic| LikeC4Diagnostic {
                    kind: LikeC4DiagnosticKind::Model,
                    path: normalize_diagnostic_path(root, &diagnostic.file, &source_paths),
                    line: diagnostic.line.map(|line| line + 1),
                    message: diagnostic.message,
                }),
        );
    result.diagnostics.sort_by(|left, right| {
        (&left.path, left.line.unwrap_or(0), &left.message).cmp(&(
            &right.path,
            right.line.unwrap_or(0),
            &right.message,
        ))
    });
    if response.valid && result.diagnostics.is_empty() {
        result.model = response.model;
    }
    result
}

fn read_capped(mut reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut stored = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(stored);
        }
        let remaining = (MAX_BRIDGE_OUTPUT + 1).saturating_sub(stored.len());
        stored.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn bridge_reader_panic(
    mut result: LikeC4Workspace,
    workspace: String,
    stream: &str,
) -> LikeC4Workspace {
    result.diagnostics.push(LikeC4Diagnostic {
        kind: LikeC4DiagnosticKind::Runtime,
        path: workspace,
        line: None,
        message: format!("LikeC4 bridge {stream} reader failed"),
    });
    result
}

fn normalize_diagnostic_path(root: &Path, path: &str, sources: &[String]) -> String {
    let normalized = path.replace('\\', "/");
    if let Ok(relative) = Path::new(path).strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }
    sources
        .iter()
        .find(|source| normalized.ends_with(source.as_str()))
        .cloned()
        .unwrap_or(normalized)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
