use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use ast_grep_config::{DeserializeEnv, SerializableRuleCore};
#[cfg(test)]
use ast_grep_core::Matcher;
use ast_grep_core::meta_var::MetaVariable;
use ast_grep_core::{Doc, NodeMatch, Pattern};
use ast_grep_language::{Language, LanguageExt, SupportLang};

use crate::diagnostic::SourceLocation;
use crate::source::read_source_to_string_from;
use crate::vault::{PolicyPattern, Vault};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PatternSource<'a> {
    Pattern(&'a str),
    Rule(&'a str),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct StructuralMatch {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) range: String,
    pub(crate) text: String,
    pub(crate) captures: BTreeMap<String, String>,
    pub(crate) location: Option<SourceLocation>,
}

enum CompiledMatcher {
    Pattern(Pattern),
    Rule(ast_grep_config::RuleCore),
}

pub(crate) struct CompiledPolicy {
    language: SupportLang,
    matcher: CompiledMatcher,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum PolicyCompileError {
    MissingDefinition,
    MissingLanguage,
    AmbiguousBody,
    MissingBody,
    InvalidPattern(String),
    InvalidRule(String),
}

impl fmt::Display for PolicyCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefinition => {
                formatter.write_str("policy pattern must declare language and pattern or rule")
            }
            Self::MissingLanguage => {
                formatter.write_str("inline policy pattern must declare a language")
            }
            Self::AmbiguousBody => formatter
                .write_str("inline policy pattern must declare either pattern or rule, not both"),
            Self::MissingBody => {
                formatter.write_str("inline policy pattern must declare pattern or rule")
            }
            Self::InvalidPattern(message) | Self::InvalidRule(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl From<PolicyCompileError> for CrivError {
    fn from(error: PolicyCompileError) -> Self {
        Self::new(error.to_string())
    }
}

pub(crate) struct PolicyScanRequest<'a> {
    pub(crate) key: usize,
    pub(crate) policy: &'a CompiledPolicy,
    pub(crate) paths: &'a BTreeSet<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) policy_compilations: usize,
    pub(crate) ast_parses: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        policy_compilations: 0,
        ast_parses: 0,
    }) };
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_batch_parse_count() {
    reset_work_counts();
}

#[cfg(test)]
pub(crate) fn batch_parse_count() -> usize {
    work_counts().ast_parses
}

pub(crate) fn compile_policy(
    policy: &PolicyPattern,
) -> std::result::Result<CompiledPolicy, PolicyCompileError> {
    let (source, language) = policy_source(policy)?;
    let language = parse_language(language).map_err(|error| match source {
        PatternSource::Pattern(_) => PolicyCompileError::InvalidPattern(error.to_string()),
        PatternSource::Rule(_) => PolicyCompileError::InvalidRule(error.to_string()),
    })?;
    #[cfg(test)]
    record_work(|counts| counts.policy_compilations += 1);
    let matcher = compile(source, language).map_err(|error| match source {
        PatternSource::Pattern(_) => PolicyCompileError::InvalidPattern(error.to_string()),
        PatternSource::Rule(_) => PolicyCompileError::InvalidRule(error.to_string()),
    })?;
    Ok(CompiledPolicy { language, matcher })
}

pub(crate) fn find_policies_batch(
    vault: &Vault,
    requests: &[PolicyScanRequest<'_>],
) -> Result<BTreeMap<usize, Vec<StructuralMatch>>> {
    let mut rows_by_key = BTreeMap::<usize, Vec<StructuralMatch>>::new();
    for request in requests {
        rows_by_key.entry(request.key).or_default();
    }

    for source_file in vault.source_files() {
        let Some(language) = SupportLang::from_path(source_file) else {
            continue;
        };
        let requests = requests
            .iter()
            .filter(|request| {
                request.policy.language == language && request.paths.contains(source_file)
            })
            .collect::<Vec<_>>();
        if requests.is_empty() {
            continue;
        }

        let contents: Arc<str> = Arc::from(read_source_to_string_from(
            vault.repository_files(),
            source_file,
        )?);
        #[cfg(test)]
        record_work(|counts| counts.ast_parses += 1);
        let ast = language.ast_grep(contents.as_ref());
        let root = ast.root();
        for request in requests {
            let rows = rows_by_key.entry(request.key).or_default();
            match &request.policy.matcher {
                CompiledMatcher::Pattern(pattern) => {
                    rows.extend(
                        root.find_all(pattern)
                            .map(|matched| row_from_match(source_file, &matched, contents.clone())),
                    );
                }
                CompiledMatcher::Rule(rule) => {
                    rows.extend(
                        root.find_all(rule)
                            .map(|matched| row_from_match(source_file, &matched, contents.clone())),
                    );
                }
            }
        }
    }

    for rows in rows_by_key.values_mut() {
        sort_matches(rows);
    }
    Ok(rows_by_key)
}

fn sort_matches(rows: &mut Vec<StructuralMatch>) {
    rows.sort_by(|left, right| {
        (&left.path, left.line, &left.range, &left.text).cmp(&(
            &right.path,
            right.line,
            &right.range,
            &right.text,
        ))
    });
    rows.dedup_by(|left, right| {
        left.path == right.path && left.range == right.range && left.text == right.text
    });
}

fn policy_source(
    policy: &PolicyPattern,
) -> std::result::Result<(PatternSource<'_>, &str), PolicyCompileError> {
    if !policy.has_inline_definition() {
        return Err(PolicyCompileError::MissingDefinition);
    }
    let Some(language) = policy
        .language
        .as_deref()
        .map(str::trim)
        .filter(|language| !language.is_empty())
    else {
        return Err(PolicyCompileError::MissingLanguage);
    };

    match (policy.pattern.as_deref(), policy.rule.as_deref()) {
        (Some(_), Some(_)) => Err(PolicyCompileError::AmbiguousBody),
        (None, None) => Err(PolicyCompileError::MissingBody),
        (Some(pattern), None) => Ok((PatternSource::Pattern(pattern), language)),
        (None, Some(rule)) => Ok((PatternSource::Rule(rule), language)),
    }
}

fn compile(source: PatternSource<'_>, language: SupportLang) -> Result<CompiledMatcher> {
    match source {
        PatternSource::Pattern(pattern) => Pattern::try_new(pattern, language)
            .map(CompiledMatcher::Pattern)
            .map_err(|err| {
                CrivError::new(format!(
                    "failed to compile ast-grep pattern for {language}: {err}"
                ))
            }),
        PatternSource::Rule(rule) => {
            let yaml = normalize_rule(rule);
            let serial: SerializableRuleCore = ast_grep_config::from_str(&yaml).map_err(|err| {
                CrivError::new(format!(
                    "failed to parse ast-grep rule for {language}: {err}"
                ))
            })?;
            serial
                .get_matcher(DeserializeEnv::new(language))
                .map(CompiledMatcher::Rule)
                .map_err(|err| {
                    CrivError::new(format!(
                        "failed to compile ast-grep rule for {language}: {err}"
                    ))
                })
        }
    }
}

#[cfg(test)]
fn scan_source<M: Matcher>(
    source_file: &str,
    language: SupportLang,
    contents: &str,
    matcher: &M,
) -> Vec<StructuralMatch> {
    let contents: Arc<str> = Arc::from(contents);
    let ast = language.ast_grep(contents.as_ref());
    ast.root()
        .find_all(matcher)
        .map(|matched| row_from_match(source_file, &matched, contents.clone()))
        .collect()
}

fn row_from_match<D: Doc>(
    source_file: &str,
    matched: &NodeMatch<'_, D>,
    source: Arc<str>,
) -> StructuralMatch {
    let node = matched.get_node();
    let start = node.start_pos();
    let end = node.end_pos();
    let line = start.line() + 1;
    StructuralMatch {
        path: source_file.to_string(),
        line,
        range: format!(
            "L{}:C{}-L{}:C{}",
            line,
            start.column(node) + 1,
            end.line() + 1,
            end.column(node) + 1
        ),
        text: node.text().trim().to_string(),
        captures: capture_map(matched),
        location: SourceLocation::new(source, node.range()),
    }
}

fn capture_map<D: Doc>(matched: &NodeMatch<'_, D>) -> BTreeMap<String, String> {
    let env = matched.get_env();
    let mut captures = BTreeMap::new();
    for variable in env.get_matched_variables() {
        match variable {
            MetaVariable::Capture(name, _) => {
                if let Some(node) = env.get_match(&name) {
                    captures.insert(name, node.text().to_string());
                }
            }
            MetaVariable::MultiCapture(name) => {
                let values = env
                    .get_multiple_matches(&name)
                    .into_iter()
                    .map(|node| node.text().to_string())
                    .collect::<Vec<_>>();
                captures.insert(name, values.join("\n"));
            }
            MetaVariable::Multiple | MetaVariable::Dropped(_) => {}
        }
    }
    captures
}

fn normalize_rule(rule: &str) -> String {
    let trimmed = rule.trim();
    if trimmed.starts_with("rule:") {
        return trimmed.to_string();
    }
    format!("rule:\n{}", indent_yaml(trimmed))
}

fn indent_yaml(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_language(language: &str) -> Result<SupportLang> {
    language
        .parse()
        .map_err(|err| CrivError::new(format!("unsupported ast-grep language `{language}`: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ast_grep_pattern_returns_ranges_and_captures() {
        let matches = scan_source(
            "src/main.rs",
            SupportLang::Rust,
            "fn main() {\n  println!(\"hi\");\n}\nfn helper() {}\n",
            &Pattern::try_new("fn $NAME() { $$$ }", SupportLang::Rust).unwrap(),
        );

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 1);
        assert_eq!(matches[0].range, "L1:C1-L3:C2");
        let exact = matches[0].location.as_ref().unwrap().lsp_range();
        assert_eq!((exact.start.line, exact.start.character), (0, 0));
        assert_eq!((exact.end.line, exact.end.character), (2, 1));
        assert_eq!(
            matches[0].captures.get("NAME").map(String::as_str),
            Some("main")
        );
    }

    #[test]
    fn ast_grep_rule_supports_composite_yaml() {
        let rule = compile(
            PatternSource::Rule(
                r#"
all:
  - pattern: println!($$$ARGS)
  - inside:
      pattern: fn $NAME() { $$$ }
      stopBy: end
"#,
            ),
            SupportLang::Rust,
        )
        .unwrap();
        let CompiledMatcher::Rule(rule) = rule else {
            panic!("expected rule matcher");
        };
        let matches = scan_source(
            "src/main.rs",
            SupportLang::Rust,
            "fn main() {\n  println!(\"hi\");\n}\n",
            &rule,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 2);
        assert_eq!(
            matches[0].captures.get("ARGS").map(String::as_str),
            Some("\"hi\"")
        );
    }

    #[test]
    fn batch_returns_all_expected_policy_rows() {
        let (_temp, vault) = policy_fixture();
        let function_policy = policy("rust", "fn $NAME() { $$$ }");
        let struct_policy = policy("rust", "struct $NAME;");
        let function_compiled = compile_policy(&function_policy).unwrap();
        let struct_compiled = compile_policy(&struct_policy).unwrap();
        let policy_paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = vec![
            PolicyScanRequest {
                key: 0,
                policy: &function_compiled,
                paths: &policy_paths,
            },
            PolicyScanRequest {
                key: 1,
                policy: &struct_compiled,
                paths: &policy_paths,
            },
        ];

        let batch = find_policies_batch(&vault, &requests).unwrap();

        assert_eq!(
            batch
                .get(&0)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/left.rs", "src/right.rs"]
        );
        assert_eq!(
            batch
                .get(&1)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/left.rs", "src/right.rs"]
        );
    }

    #[test]
    fn batch_respects_per_pattern_scopes() {
        let (_temp, vault) = policy_fixture();
        let function_policy = policy("rust", "fn $NAME() { $$$ }");
        let left_paths = BTreeSet::from(["src/left.rs".to_string()]);
        let right_paths = BTreeSet::from(["src/right.rs".to_string()]);
        reset_work_counts();
        let function_compiled = compile_policy(&function_policy).unwrap();
        let requests = vec![
            PolicyScanRequest {
                key: 0,
                policy: &function_compiled,
                paths: &left_paths,
            },
            PolicyScanRequest {
                key: 1,
                policy: &function_compiled,
                paths: &right_paths,
            },
        ];

        let batch = find_policies_batch(&vault, &requests).unwrap();

        assert_eq!(
            batch
                .get(&0)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/left.rs"]
        );
        assert_eq!(
            batch
                .get(&1)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/right.rs"]
        );
        assert_eq!(
            work_counts(),
            WorkCounts {
                policy_compilations: 1,
                ast_parses: 2,
            },
            "the policy is compiled once and each affected source is parsed once"
        );
    }

    #[test]
    fn batch_skips_non_matching_language() {
        let (_temp, vault) = policy_fixture();
        let python_policy = policy("python", "def $NAME($$$): $$$");
        let python_compiled = compile_policy(&python_policy).unwrap();
        let paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = vec![PolicyScanRequest {
            key: 0,
            policy: &python_compiled,
            paths: &paths,
        }];

        let batch = find_policies_batch(&vault, &requests).unwrap();

        assert!(batch.get(&0).unwrap().is_empty());
    }

    #[test]
    fn elixir_language_names_extensions_patterns_and_rules_have_parity() {
        assert_eq!(parse_language("elixir").unwrap(), SupportLang::Elixir);
        assert_eq!(parse_language("ex").unwrap(), SupportLang::Elixir);
        assert_eq!(
            SupportLang::from_path("lib/sample.ex"),
            Some(SupportLang::Elixir)
        );
        assert_eq!(
            SupportLang::from_path("test/sample_test.exs"),
            Some(SupportLang::Elixir)
        );

        let source = r#"
defmodule Sample do
  def run(value) when is_integer(value) do
    IO.inspect(value)
  end
end
"#;
        let pattern = Pattern::try_new(
            r#"
def $FUNC($$$ARGS) when $GUARDS do
  $$$BODY
end
"#,
            SupportLang::Elixir,
        )
        .unwrap();
        let matches = scan_source("lib/sample.ex", SupportLang::Elixir, source, &pattern);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].captures.get("FUNC").map(String::as_str),
            Some("run")
        );

        let CompiledMatcher::Rule(rule) = compile(
            PatternSource::Rule("pattern: IO.inspect($VALUE)"),
            SupportLang::Elixir,
        )
        .unwrap() else {
            panic!("expected an Elixir rule matcher");
        };
        let rule_matches = scan_source("test/sample_test.exs", SupportLang::Elixir, source, &rule);
        assert_eq!(rule_matches.len(), 1);
    }

    #[test]
    fn elixir_partial_files_and_invalid_patterns_return_results_without_panics() {
        let pattern = Pattern::try_new("IO.inspect($VALUE)", SupportLang::Elixir).unwrap();
        let matches = scan_source(
            "lib/partial.ex",
            SupportLang::Elixir,
            "defmodule Partial do\n  IO.inspect(:before)\n  def broken(\n  IO.inspect(:after)\nend\n",
            &pattern,
        );
        assert!(!matches.is_empty());

        let invalid = policy("elixir", "def (");
        assert!(matches!(
            compile_policy(&invalid),
            Err(PolicyCompileError::InvalidPattern(_))
        ));
    }

    #[test]
    fn elixir_batch_scans_both_extensions_once_and_respects_scope() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("test")).unwrap();
        fs::write(
            root.join("criv.toml"),
            "[source]\nroots = [\"lib\", \"test\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("lib/sample.ex"),
            "defmodule Sample do\n  def run(value), do: IO.inspect(value)\nend\n",
        )
        .unwrap();
        fs::write(
            root.join("test/sample_test.exs"),
            "defmodule SampleTest do\n  def run(value), do: IO.inspect(value)\nend\n",
        )
        .unwrap();
        let vault = Vault::load(root).unwrap();
        let compiled = compile_policy(&policy("ex", "IO.inspect($VALUE)")).unwrap();
        let paths = BTreeSet::from([
            "lib/sample.ex".to_string(),
            "test/sample_test.exs".to_string(),
        ]);
        reset_work_counts();
        let rows = find_policies_batch(
            &vault,
            &[PolicyScanRequest {
                key: 0,
                policy: &compiled,
                paths: &paths,
            }],
        )
        .unwrap();

        assert_eq!(
            rows[&0]
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["lib/sample.ex", "test/sample_test.exs"]
        );
        assert_eq!(work_counts().ast_parses, 2);
    }

    fn policy(language: &str, pattern: &str) -> PolicyPattern {
        PolicyPattern {
            id: Some("test".to_string()),
            line: 1,
            language: Some(language.to_string()),
            pattern: Some(pattern.to_string()),
            rule: None,
            message: None,
        }
    }

    fn policy_fixture() -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/left.rs"), "fn left() {}\nstruct Left;\n").unwrap();
        fs::write(root.join("src/right.rs"), "fn right() {}\nstruct Right;\n").unwrap();
        let vault = Vault::load(root).unwrap();
        (temp, vault)
    }
}
