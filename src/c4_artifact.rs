use std::path::Path;

use crate::Result;
use crate::c4::{self, C4Diagram, C4Level};
use crate::util::{read_to_string, strip_prefix};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum C4ArtifactFormat {
    Mermaid,
    Dot,
}

impl C4ArtifactFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Mermaid => "mermaid",
            Self::Dot => "dot",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum C4ArtifactLevel {
    Context,
    Container,
    Component,
    Code,
}

impl C4ArtifactLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Container => "container",
            Self::Component => "component",
            Self::Code => "code",
        }
    }

    pub(crate) fn c4_level(self) -> Option<C4Level> {
        match self {
            Self::Context => Some(C4Level::Context),
            Self::Container => Some(C4Level::Container),
            Self::Component => Some(C4Level::Component),
            Self::Code => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct C4Artifact {
    pub(crate) path: std::path::PathBuf,
    pub(crate) rel_path: String,
    pub(crate) format: Option<C4ArtifactFormat>,
    pub(crate) level: Option<C4ArtifactLevel>,
    pub(crate) directives: Vec<C4Directive>,
    pub(crate) diagrams: Vec<C4Diagram>,
    pub(crate) diagnostics: Vec<C4ArtifactDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Directive {
    pub(crate) key: String,
    pub(crate) value: Option<String>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4ArtifactDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) line: Option<usize>,
    pub(crate) message: String,
}

pub(crate) fn parse_file(root: &Path, docs_path: &Path, path: &Path) -> Result<C4Artifact> {
    let contents = read_to_string(path)?;
    Ok(parse_contents(root, docs_path, path, &contents))
}

pub(crate) fn parse_contents(
    root: &Path,
    docs_path: &Path,
    path: &Path,
    contents: &str,
) -> C4Artifact {
    let rel_path = strip_prefix(path, root);
    let mut directives = Vec::new();
    let mut diagnostics = Vec::new();
    let level = filename_level(path);
    if level.is_none() {
        diagnostics.push(C4ArtifactDiagnostic {
            code: "missing-c4-level",
            line: None,
            message:
                ".c4 filename must include one of: context, container, component, components, code"
                    .into(),
        });
    }

    let mut asserted_format = None;
    for (line, key, value) in directive_comments(contents) {
        match key.as_str() {
            "format" => match value.as_deref().and_then(parse_format) {
                Some(format) => {
                    if asserted_format.is_some_and(|existing| existing != format) {
                        diagnostics.push(C4ArtifactDiagnostic {
                            code: "duplicate-c4-format",
                            line: Some(line),
                            message: "conflicting criv:format directives".into(),
                        });
                    }
                    asserted_format = Some(format);
                    directives.push(C4Directive { key, value, line });
                }
                None => {
                    diagnostics.push(C4ArtifactDiagnostic {
                        code: "invalid-c4-format",
                        line: Some(line),
                        message: "criv:format must be one of: mermaid, mermaid-c4, dot, graphviz"
                            .to_string(),
                    });
                    directives.push(C4Directive { key, value, line });
                }
            },
            "generated" => {
                if let Some(value) = value.as_deref()
                    && !matches!(value, "true" | "false")
                {
                    diagnostics.push(C4ArtifactDiagnostic {
                        code: "invalid-c4-generated",
                        line: Some(line),
                        message: "criv:generated must be true or false".into(),
                    });
                }
                directives.push(C4Directive { key, value, line });
            }
            "source" => {}
            _ => {
                diagnostics.push(C4ArtifactDiagnostic {
                    code: "unknown-c4-directive",
                    line: Some(line),
                    message: format!("unknown .c4 directive `criv:{key}`"),
                });
                directives.push(C4Directive { key, value, line });
            }
        }
    }

    let inferred_format = infer_format(contents);
    let format = inferred_format.or(asserted_format);
    match (asserted_format, inferred_format) {
        (Some(asserted), Some(inferred)) if asserted != inferred => {
            diagnostics.push(C4ArtifactDiagnostic {
                code: "mismatched-c4-format",
                line: first_meaningful_line(contents).map(|(line, _)| line),
                message: format!(
                    "criv:format {} does not match inferred {} content",
                    asserted.as_str(),
                    inferred.as_str()
                ),
            });
        }
        (None, None) => diagnostics.push(C4ArtifactDiagnostic {
            code: "unknown-c4-format",
            line: first_non_empty_line(contents),
            message: ".c4 content must start with Mermaid C4 or DOT syntax".into(),
        }),
        _ => {}
    }

    if format == Some(C4ArtifactFormat::Dot)
        && level.is_some_and(|level| level != C4ArtifactLevel::Code)
    {
        diagnostics.push(C4ArtifactDiagnostic {
            code: "invalid-c4-level",
            line: None,
            message: "DOT .c4 artifacts are expected to be code-level files".into(),
        });
    }

    let diagrams = match format {
        Some(C4ArtifactFormat::Mermaid) => {
            parse_mermaid_artifact(contents, level, &mut diagnostics)
        }
        Some(C4ArtifactFormat::Dot) => Vec::new(),
        None => Vec::new(),
    };

    let doc_rel_path = strip_prefix(path, docs_path);
    C4Artifact {
        path: path.to_path_buf(),
        rel_path: if rel_path.is_empty() {
            doc_rel_path
        } else {
            rel_path
        },
        format,
        level,
        directives,
        diagrams,
        diagnostics,
    }
}

fn parse_mermaid_artifact(
    contents: &str,
    filename_level: Option<C4ArtifactLevel>,
    diagnostics: &mut Vec<C4ArtifactDiagnostic>,
) -> Vec<C4Diagram> {
    let Some(diagram) = c4::parse_mermaid_diagram(0, contents) else {
        diagnostics.push(C4ArtifactDiagnostic {
            code: "invalid-c4-mermaid",
            line: first_meaningful_line(contents).map(|(line, _)| line),
            message:
                "Mermaid .c4 file must contain a C4Context, C4Container, or C4Component diagram"
                    .into(),
        });
        return Vec::new();
    };

    if let Some(expected) = filename_level {
        match expected.c4_level() {
            Some(expected) if expected != diagram.level => {
                diagnostics.push(C4ArtifactDiagnostic {
                    code: "mismatched-c4-level",
                    line: Some(diagram.line),
                    message: format!(
                        "filename level `{}` does not match Mermaid header `{}`",
                        expected.as_str(),
                        diagram.level.as_str()
                    ),
                });
            }
            None => diagnostics.push(C4ArtifactDiagnostic {
                code: "mismatched-c4-level",
                line: Some(diagram.line),
                message: "Code-level .c4 files cannot contain Mermaid C4Context/C4Container/C4Component headers"
                    .into(),
            }),
            _ => {}
        }
    }

    vec![diagram]
}

fn directive_comments(contents: &str) -> Vec<(usize, String, Option<String>)> {
    contents
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let comment = comment_text(line.trim())?;
            let directive = comment.strip_prefix("criv:")?.trim();
            let (key, value) = directive
                .split_once(char::is_whitespace)
                .map(|(key, value)| (key.trim(), Some(value.trim())))
                .unwrap_or((directive, None));
            if key.is_empty() {
                return None;
            }
            Some((
                index + 1,
                key.to_string(),
                value.filter(|value| !value.is_empty()).map(str::to_string),
            ))
        })
        .collect()
}

fn infer_format(contents: &str) -> Option<C4ArtifactFormat> {
    let (_, line) = first_meaningful_line(contents)?;
    let normalized = line.trim_start();
    if matches!(normalized, "C4Context" | "C4Container" | "C4Component") {
        return Some(C4ArtifactFormat::Mermaid);
    }
    if normalized == "digraph"
        || normalized == "graph"
        || normalized.starts_with("digraph ")
        || normalized.starts_with("graph ")
        || normalized.starts_with("strict digraph")
        || normalized.starts_with("strict graph")
    {
        return Some(C4ArtifactFormat::Dot);
    }
    None
}

fn first_meaningful_line(contents: &str) -> Option<(usize, &str)> {
    contents.lines().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        (!trimmed.is_empty() && comment_text(trimmed).is_none()).then_some((index + 1, trimmed))
    })
}

fn first_non_empty_line(contents: &str) -> Option<usize> {
    contents
        .lines()
        .position(|line| !line.trim().is_empty())
        .map(|index| index + 1)
}

fn comment_text(line: &str) -> Option<&str> {
    line.strip_prefix("%%")
        .or_else(|| line.strip_prefix("//"))
        .or_else(|| line.strip_prefix('#'))
        .map(str::trim)
}

fn parse_format(value: &str) -> Option<C4ArtifactFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mermaid" | "mermaid-c4" => Some(C4ArtifactFormat::Mermaid),
        "dot" | "graphviz" => Some(C4ArtifactFormat::Dot),
        _ => None,
    }
}

fn filename_level(path: &Path) -> Option<C4ArtifactLevel> {
    let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
    stem.split(|ch: char| !ch.is_ascii_alphanumeric())
        .find_map(|token| match token {
            "context" => Some(C4ArtifactLevel::Context),
            "container" | "containers" => Some(C4ArtifactLevel::Container),
            "component" | "components" => Some(C4ArtifactLevel::Component),
            "code" => Some(C4ArtifactLevel::Code),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    struct ExpectedDiagnostic {
        code: String,
        line: Option<usize>,
    }

    #[test]
    fn infers_mermaid_format_and_filename_level() {
        let artifact = parse_test_artifact(
            "docs/architecture/02-container.c4",
            r#"
%% criv:generated false
C4Container
Container(cli, "criv CLI", "Rust", "Validates the vault")
"#,
        );

        assert_eq!(artifact.format, Some(C4ArtifactFormat::Mermaid));
        assert_eq!(artifact.level, Some(C4ArtifactLevel::Container));
        assert_eq!(artifact.diagrams.len(), 1);
        assert!(artifact.diagnostics.is_empty());
    }

    #[test]
    fn infers_dot_format() {
        let artifact = parse_test_artifact(
            "docs/architecture/04-code.c4",
            r#"
// criv:generated true
digraph criv_code {
  cli -> vault;
}
"#,
        );

        assert_eq!(artifact.format, Some(C4ArtifactFormat::Dot));
        assert_eq!(artifact.level, Some(C4ArtifactLevel::Code));
        assert!(artifact.diagrams.is_empty());
        assert!(artifact.diagnostics.is_empty());
    }

    #[test]
    fn reports_mismatched_format_assertion() {
        let artifact = parse_test_artifact(
            "docs/architecture/01-context.c4",
            r#"
%% criv:format dot
C4Context
Person(user, "User", "Uses criv")
"#,
        );

        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|diag| diag.code == "mismatched-c4-format")
        );
    }

    #[test]
    fn reports_missing_filename_level() {
        let artifact = parse_test_artifact(
            "docs/architecture/diagram.c4",
            r#"
C4Context
Person(user, "User", "Uses criv")
"#,
        );

        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|diag| diag.code == "missing-c4-level")
        );
    }

    #[test]
    fn reports_mermaid_header_filename_level_mismatch() {
        let artifact = parse_test_artifact(
            "docs/architecture/01-context.c4",
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates the vault")
"#,
        );

        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|diag| diag.code == "mismatched-c4-level")
        );
    }

    #[test]
    fn shared_c4_fixtures_match_expected_diagnostics() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/c4");
        let expected: std::collections::BTreeMap<String, Vec<ExpectedDiagnostic>> =
            serde_json::from_str(&std::fs::read_to_string(fixtures.join("expected.json")).unwrap())
                .unwrap();

        for (fixture, expected_diagnostics) in expected {
            let contents = std::fs::read_to_string(fixtures.join(&fixture)).unwrap();
            let artifact = parse_test_artifact(&fixture, &contents);
            let mut actual = artifact
                .diagnostics
                .into_iter()
                .map(|diagnostic| ExpectedDiagnostic {
                    code: diagnostic.code.to_string(),
                    line: diagnostic.line,
                })
                .collect::<Vec<_>>();
            actual.sort();

            assert_eq!(actual, expected_diagnostics, "diagnostics for {fixture}");
        }
    }

    fn parse_test_artifact(path: &str, contents: &str) -> C4Artifact {
        let root = PathBuf::from(".");
        let docs = PathBuf::from("docs");
        parse_contents(&root, &docs, &PathBuf::from(path), contents)
    }
}
