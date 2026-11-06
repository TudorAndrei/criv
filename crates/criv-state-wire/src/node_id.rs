//! Graph node identity text and node location text.
//!
//! A node id is `<prefix>:<body>`. A node path is a repository-relative path,
//! optionally followed by a `#L` line location.

use crate::source_identity::SourceIdentity;

/// The prefix family of a published graph node id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeIdKind {
    Note,
    Code,
    Symbol,
    Pattern,
    Import,
    ExternalCall,
    ExternalModule,
    DynamicCall,
}

impl NodeIdKind {
    const ALL: [Self; 8] = [
        Self::Note,
        Self::Code,
        Self::Symbol,
        Self::Pattern,
        Self::Import,
        Self::ExternalCall,
        Self::ExternalModule,
        Self::DynamicCall,
    ];

    const fn prefix(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Code => "code",
            Self::Symbol => "symbol",
            Self::Pattern => "pattern",
            Self::Import => "import",
            Self::ExternalCall => "external-call",
            Self::ExternalModule => "external-module",
            Self::DynamicCall => "dynamic-call",
        }
    }

    fn id(self, body: &str) -> String {
        format!("{}:{body}", self.prefix())
    }
}

/// Split a node id into its kind and body.
#[must_use]
pub fn parse(id: &str) -> Option<(NodeIdKind, &str)> {
    let (prefix, body) = id.split_once(':')?;
    NodeIdKind::ALL
        .into_iter()
        .find(|kind| kind.prefix() == prefix)
        .map(|kind| (kind, body))
}

/// Return the Source identity text of a `code:` or `symbol:` node id.
#[must_use]
pub fn source_target(id: &str) -> Option<&str> {
    match parse(id)? {
        (NodeIdKind::Code | NodeIdKind::Symbol, body) => Some(body),
        _ => None,
    }
}

#[must_use]
pub fn note(id: &str) -> String {
    NodeIdKind::Note.id(id)
}

/// The id of one heading inside the note node `note_id`.
#[must_use]
pub fn note_heading(note_id: &str, slug: &str) -> String {
    format!("{note_id}#{slug}")
}

#[must_use]
pub fn code(path: &str) -> String {
    NodeIdKind::Code.id(&SourceIdentity::file(path).to_string())
}

/// The id of a symbol node; `identity` is Source identity text `path#selector`.
#[must_use]
pub fn symbol(identity: &str) -> String {
    NodeIdKind::Symbol.id(identity)
}

#[must_use]
pub fn pattern(id: &str) -> String {
    NodeIdKind::Pattern.id(id)
}

#[must_use]
pub fn import(path: &str, module: &str) -> String {
    NodeIdKind::Import.id(&format!("{path}:{module}"))
}

/// The id of a directive that keeps its kind and source site.
#[must_use]
pub fn directive(path: &str, kind: &str, module: &str, site: usize) -> String {
    NodeIdKind::Import.id(&format!("{path}:{kind}:{module}:{site}"))
}

#[must_use]
pub fn external_call(target: &str) -> String {
    NodeIdKind::ExternalCall.id(target)
}

#[must_use]
pub fn external_module(target: &str) -> String {
    NodeIdKind::ExternalModule.id(target)
}

#[must_use]
pub fn dynamic_call(id: &str) -> String {
    NodeIdKind::DynamicCall.id(id)
}

/// The node path of one source line.
#[must_use]
pub fn line_location(path: &str, line: usize) -> String {
    format!("{path}#L{line}")
}

/// The node path of an inclusive source line range.
#[must_use]
pub fn range_location(path: &str, start: usize, end: usize) -> String {
    format!("{path}#L{start}-L{end}")
}

/// The node path of a Markdown heading line and level.
#[must_use]
pub fn heading_location(path: &str, line: usize, level: usize) -> String {
    format!("{path}#L{line}:H{level}")
}

/// Return the line location of a node path, starting at its `L`.
#[must_use]
pub fn line_suffix(location: &str) -> Option<&str> {
    location
        .find("#L")
        .and_then(|index| location.get(index.saturating_add(1)..))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_and_parser_share_one_prefix_table() {
        let ids = [
            (note("ADR-0001"), NodeIdKind::Note, "ADR-0001"),
            (code("src/lib.rs"), NodeIdKind::Code, "src/lib.rs"),
            (
                symbol("src/lib.rs#fn:run"),
                NodeIdKind::Symbol,
                "src/lib.rs#fn:run",
            ),
            (
                pattern("ADR-0001/entry"),
                NodeIdKind::Pattern,
                "ADR-0001/entry",
            ),
            (
                import("lib/a.ex", "Logger"),
                NodeIdKind::Import,
                "lib/a.ex:Logger",
            ),
            (
                directive("lib/a.ex", "alias", "My.App", 3),
                NodeIdKind::Import,
                "lib/a.ex:alias:My.App:3",
            ),
            (
                external_call("IO.puts/1"),
                NodeIdKind::ExternalCall,
                "IO.puts/1",
            ),
            (external_module("Ecto"), NodeIdKind::ExternalModule, "Ecto"),
            (dynamic_call("d1"), NodeIdKind::DynamicCall, "d1"),
        ];
        for (id, kind, body) in ids {
            assert_eq!(parse(&id), Some((kind, body)), "{id}");
        }
        assert_eq!(parse("unknown:x"), None);
        assert_eq!(
            source_target("symbol:src/lib.rs#fn:run"),
            Some("src/lib.rs#fn:run")
        );
        assert_eq!(source_target("note:ADR-0001"), None);
    }

    #[test]
    fn line_suffix_reads_every_location_form() {
        assert_eq!(line_suffix(&line_location("a.rs", 4)), Some("L4"));
        assert_eq!(line_suffix(&range_location("a.rs", 1, 9)), Some("L1-L9"));
        assert_eq!(line_suffix(&heading_location("d.md", 3, 2)), Some("L3:H2"));
        assert_eq!(line_suffix("src/lib.rs"), None);
    }
}
