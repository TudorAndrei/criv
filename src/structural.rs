use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use ast_grep_config::{DeserializeEnv, SerializableRuleCore};
use ast_grep_core::meta_var::MetaVariable;
use ast_grep_core::{Doc, NodeMatch, Pattern};
use ast_grep_language::{Language, LanguageExt, SupportLang};

use crate::Result;
use crate::diagnostic::{DiagnosticCode, SourceLocation};
use crate::repository::RepositoryFiles;
use crate::source::read_selected;
#[cfg(test)]
use crate::work::{self, Work};

/// The fields of a policy pattern that decide what it matches.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyDefinition<'a> {
    pub(crate) language: Option<&'a str>,
    pub(crate) pattern: Option<&'a str>,
    pub(crate) rule: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
enum PatternSource<'a> {
    Pattern(&'a str),
    Rule(&'a str),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StructuralMatch {
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

pub struct CompiledPolicy {
    language: SupportLang,
    matcher: CompiledMatcher,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PolicyCompileError {
    MissingDefinition,
    MissingLanguage,
    AmbiguousBody,
    MissingBody,
    Invalid(String),
}

impl PolicyCompileError {
    pub(crate) const fn code(&self) -> DiagnosticCode {
        match self {
            Self::MissingDefinition => DiagnosticCode::MissingPolicyPatternDefinition,
            Self::MissingLanguage => DiagnosticCode::MissingPolicyPatternLanguage,
            Self::AmbiguousBody => DiagnosticCode::AmbiguousPolicyPatternBody,
            Self::MissingBody => DiagnosticCode::MissingPolicyPatternBody,
            Self::Invalid(_) => DiagnosticCode::InvalidPolicyPattern,
        }
    }
}

/// The predicate that follows the pattern's name, as in "policy pattern `x` must ...".
impl fmt::Display for PolicyCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefinition => {
                formatter.write_str("must declare language and pattern or rule")
            }
            Self::MissingLanguage => formatter.write_str("must declare a language"),
            Self::AmbiguousBody => {
                formatter.write_str("must declare either pattern or rule, not both")
            }
            Self::MissingBody => formatter.write_str("must declare pattern or rule"),
            Self::Invalid(message) => write!(formatter, "does not compile: {message}"),
        }
    }
}

pub struct PolicyScanRequest<'a> {
    pub(crate) policy: &'a CompiledPolicy,
    pub(crate) paths: &'a BTreeSet<String>,
}

pub fn compile_policy(
    definition: PolicyDefinition<'_>,
) -> std::result::Result<CompiledPolicy, PolicyCompileError> {
    let (source, name) = policy_source(definition)?;
    let language = name.parse::<SupportLang>().map_err(|error| {
        PolicyCompileError::Invalid(format!("unsupported ast-grep language `{name}`: {error}"))
    })?;
    #[cfg(test)]
    work::record(Work::PolicyCompilation);
    let matcher = compile(source, language)?;
    Ok(CompiledPolicy { language, matcher })
}

/// The matches of each request, in request order. Each file is read and
/// parsed at most once for all the requests whose language and paths
/// include it.
pub fn find_policies_batch(
    files: &RepositoryFiles,
    requests: &[PolicyScanRequest<'_>],
) -> Result<Vec<Vec<StructuralMatch>>> {
    let mut rows = vec![Vec::new(); requests.len()];
    let paths = requests
        .iter()
        .flat_map(|request| request.paths)
        .collect::<BTreeSet<_>>();
    for source_file in paths {
        let Some(language) = SupportLang::from_path(source_file) else {
            continue;
        };
        let scans = requests
            .iter()
            .zip(&mut rows)
            .filter(|(request, _)| {
                request.policy.language == language && request.paths.contains(source_file)
            })
            .collect::<Vec<_>>();
        if scans.is_empty() {
            continue;
        }

        let contents: Arc<str> =
            Arc::from(String::from_utf8_lossy(&read_selected(files, source_file)?));
        #[cfg(test)]
        work::record(Work::AstParse);
        let ast = language.ast_grep(contents.as_ref());
        let root = ast.root();
        for (request, rows) in scans {
            let row = |matched| row_from_match(source_file, &matched, contents.clone());
            match &request.policy.matcher {
                CompiledMatcher::Pattern(pattern) => rows.extend(root.find_all(pattern).map(row)),
                CompiledMatcher::Rule(rule) => rows.extend(root.find_all(rule).map(row)),
            }
        }
    }

    for rows in &mut rows {
        sort_matches(rows);
    }
    Ok(rows)
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

const fn policy_source(
    definition: PolicyDefinition<'_>,
) -> std::result::Result<(PatternSource<'_>, &str), PolicyCompileError> {
    let PolicyDefinition {
        language,
        pattern,
        rule,
    } = definition;
    if language.is_none() && pattern.is_none() && rule.is_none() {
        return Err(PolicyCompileError::MissingDefinition);
    }
    let Some(language) = language else {
        return Err(PolicyCompileError::MissingLanguage);
    };

    match (pattern, rule) {
        (Some(_), Some(_)) => Err(PolicyCompileError::AmbiguousBody),
        (None, None) => Err(PolicyCompileError::MissingBody),
        (Some(pattern), None) => Ok((PatternSource::Pattern(pattern), language)),
        (None, Some(rule)) => Ok((PatternSource::Rule(rule), language)),
    }
}

fn compile(
    source: PatternSource<'_>,
    language: SupportLang,
) -> std::result::Result<CompiledMatcher, PolicyCompileError> {
    match source {
        PatternSource::Pattern(pattern) => Pattern::try_new(pattern, language)
            .map(CompiledMatcher::Pattern)
            .map_err(|err| {
                PolicyCompileError::Invalid(format!(
                    "failed to compile ast-grep pattern for {language}: {err}"
                ))
            }),
        PatternSource::Rule(rule) => {
            let yaml = normalize_rule(rule);
            let serial: SerializableRuleCore = ast_grep_config::from_str(&yaml).map_err(|err| {
                PolicyCompileError::Invalid(format!(
                    "failed to parse ast-grep rule for {language}: {err}"
                ))
            })?;
            serial
                .get_matcher(DeserializeEnv::new(language))
                .map(CompiledMatcher::Rule)
                .map_err(|err| {
                    PolicyCompileError::Invalid(format!(
                        "failed to compile ast-grep rule for {language}: {err}"
                    ))
                })
        }
    }
}

fn row_from_match<D: Doc>(
    source_file: &str,
    matched: &NodeMatch<'_, D>,
    source: Arc<str>,
) -> StructuralMatch {
    let node = matched.get_node();
    let start = node.start_pos();
    let end = node.end_pos();
    let line = start.line().saturating_add(1);
    StructuralMatch {
        path: source_file.to_string(),
        line,
        range: format!(
            "L{}:C{}-L{}:C{}",
            line,
            start.column(node).saturating_add(1),
            end.line().saturating_add(1),
            end.column(node).saturating_add(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ast_grep_pattern_returns_ranges_and_captures() {
        let matches = scan(
            "src/main.rs",
            "fn main() {\n  println!(\"hi\");\n}\nfn helper() {}\n",
            policy("rust", "fn $NAME() { $$$ }"),
        );

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 1);
        assert_eq!(matches[0].range, "L1:C1-L3:C2");
        let exact = matches[0].location.as_ref().unwrap().lsp_range().unwrap();
        assert_eq!((exact.start.line, exact.start.character), (0, 0));
        assert_eq!((exact.end.line, exact.end.character), (2, 1));
        assert_eq!(
            matches[0].captures.get("NAME").map(String::as_str),
            Some("main")
        );
    }

    #[test]
    fn ast_grep_rule_supports_composite_yaml() {
        let matches = scan(
            "src/main.rs",
            "fn main() {\n  println!(\"hi\");\n}\n",
            rule_policy(
                "rust",
                r#"
all:
  - pattern: println!($$$ARGS)
  - inside:
      pattern: fn $NAME() { $$$ }
      stopBy: end
"#,
            ),
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 2);
        assert_eq!(
            matches[0].captures.get("ARGS").map(String::as_str),
            Some("\"hi\"")
        );
    }

    #[test]
    fn batch_returns_rows_in_request_order() {
        let (_temp, files) = policy_fixture();
        let function_compiled = compile_policy(policy("rust", "fn $NAME() { $$$ }")).unwrap();
        let struct_compiled = compile_policy(policy("rust", "struct $NAME;")).unwrap();
        let policy_paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = [
            PolicyScanRequest {
                policy: &struct_compiled,
                paths: &policy_paths,
            },
            PolicyScanRequest {
                policy: &function_compiled,
                paths: &policy_paths,
            },
        ];

        let batch = find_policies_batch(&files, &requests).unwrap();

        assert_eq!(
            texts(&batch),
            [
                vec!["struct Left;", "struct Right;"],
                vec!["fn left() {}", "fn right() {}"]
            ]
        );
    }

    #[test]
    fn batch_respects_per_pattern_scopes() {
        let (_temp, files) = policy_fixture();
        let left_paths = BTreeSet::from(["src/left.rs".to_string()]);
        let right_paths = BTreeSet::from(["src/right.rs".to_string()]);
        work::reset();
        let function_compiled = compile_policy(policy("rust", "fn $NAME() { $$$ }")).unwrap();
        let requests = [
            PolicyScanRequest {
                policy: &function_compiled,
                paths: &left_paths,
            },
            PolicyScanRequest {
                policy: &function_compiled,
                paths: &right_paths,
            },
        ];

        let batch = find_policies_batch(&files, &requests).unwrap();

        assert_eq!(texts(&batch), [vec!["fn left() {}"], vec!["fn right() {}"]]);
        assert_eq!(
            work::counts([Work::PolicyCompilation, Work::AstParse]),
            [1, 2],
            "the policy is compiled once and each affected source is parsed once"
        );
    }

    #[test]
    fn batch_skips_non_matching_language() {
        let (_temp, files) = policy_fixture();
        let python_compiled = compile_policy(policy("python", "def $NAME($$$): $$$")).unwrap();
        let paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = [PolicyScanRequest {
            policy: &python_compiled,
            paths: &paths,
        }];

        let batch = find_policies_batch(&files, &requests).unwrap();

        assert_eq!(batch, [Vec::new()]);
    }

    #[test]
    fn unsupported_languages_do_not_compile() {
        assert!(matches!(
            compile_policy(policy("cobol", "MOVE $A TO $B")),
            Err(PolicyCompileError::Invalid(message)) if message.contains("`cobol`")
        ));
    }

    #[test]
    fn elixir_language_names_extensions_patterns_and_rules_have_parity() {
        assert_eq!(
            "elixir".parse::<SupportLang>().ok(),
            Some(SupportLang::Elixir)
        );
        assert_eq!("ex".parse::<SupportLang>().ok(), Some(SupportLang::Elixir));
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
        let matches = scan(
            "lib/sample.ex",
            source,
            policy(
                "elixir",
                r#"
def $FUNC($$$ARGS) when $GUARDS do
  $$$BODY
end
"#,
            ),
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].captures.get("FUNC").map(String::as_str),
            Some("run")
        );

        let rule_matches = scan(
            "test/sample_test.exs",
            source,
            rule_policy("elixir", "pattern: IO.inspect($VALUE)"),
        );
        assert_eq!(rule_matches.len(), 1);
    }

    #[test]
    fn elixir_partial_files_and_invalid_patterns_return_results_without_panics() {
        let matches = scan(
            "lib/partial.ex",
            "defmodule Partial do\n  IO.inspect(:before)\n  def broken(\n  IO.inspect(:after)\nend\n",
            policy("elixir", "IO.inspect($VALUE)"),
        );
        assert!(!matches.is_empty());

        assert!(matches!(
            compile_policy(policy("elixir", "def (")),
            Err(PolicyCompileError::Invalid(_))
        ));
    }

    #[test]
    fn elixir_batch_scans_both_extensions_once_and_respects_scope() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("test")).unwrap();
        fs::write(root.join("lib/other.ex"), "IO.inspect(:outside)\n").unwrap();
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
        let files = RepositoryFiles::open(root).unwrap();
        let compiled = compile_policy(policy("ex", "IO.inspect($VALUE)")).unwrap();
        let paths = BTreeSet::from([
            "lib/sample.ex".to_string(),
            "test/sample_test.exs".to_string(),
        ]);
        work::reset();
        let rows = find_policies_batch(
            &files,
            &[PolicyScanRequest {
                policy: &compiled,
                paths: &paths,
            }],
        )
        .unwrap();

        assert_eq!(
            rows[0]
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["lib/sample.ex", "test/sample_test.exs"]
        );
        assert_eq!(work::count(Work::AstParse), 2);
    }

    fn policy<'a>(language: &'a str, pattern: &'a str) -> PolicyDefinition<'a> {
        PolicyDefinition {
            language: Some(language),
            pattern: Some(pattern),
            rule: None,
        }
    }

    fn rule_policy<'a>(language: &'a str, rule: &'a str) -> PolicyDefinition<'a> {
        PolicyDefinition {
            language: Some(language),
            pattern: None,
            rule: Some(rule),
        }
    }

    fn texts(batch: &[Vec<StructuralMatch>]) -> Vec<Vec<&str>> {
        batch
            .iter()
            .map(|rows| rows.iter().map(|row| row.text.as_str()).collect())
            .collect()
    }

    fn scan(path: &str, contents: &str, policy: PolicyDefinition<'_>) -> Vec<StructuralMatch> {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, contents).unwrap();
        let files = RepositoryFiles::open(temp.path()).unwrap();
        let compiled = compile_policy(policy).unwrap();
        let paths = BTreeSet::from([path.to_string()]);
        find_policies_batch(
            &files,
            &[PolicyScanRequest {
                policy: &compiled,
                paths: &paths,
            }],
        )
        .unwrap()
        .remove(0)
    }

    fn policy_fixture() -> (TempDir, RepositoryFiles) {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/left.rs"), "fn left() {}\nstruct Left;\n").unwrap();
        fs::write(root.join("src/right.rs"), "fn right() {}\nstruct Right;\n").unwrap();
        let files = RepositoryFiles::open(root).unwrap();
        (temp, files)
    }
}
