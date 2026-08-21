//! Structured Source identity and stable wire-text conversion.

use std::fmt;

/// One file or symbol identity in published Source target text.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceIdentity {
    path: String,
    selector: Option<SourceSelector>,
}

impl SourceIdentity {
    /// Create a file identity with no selector.
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            selector: None,
        }
    }

    /// Create a symbol identity with one selector.
    pub fn symbol(path: impl Into<String>, selector: SourceSelector) -> Self {
        Self {
            path: path.into(),
            selector: Some(selector),
        }
    }

    /// Parse published Source identity text.
    ///
    /// Unknown and malformed selector forms remain exact opaque text.
    pub fn parse(text: &str) -> Self {
        match text.split_once('#') {
            Some((path, selector)) => Self::symbol(path, SourceSelector::parse(selector)),
            None => Self::file(text),
        }
    }

    /// Return the repository-relative source path.
    #[cfg(test)]
    fn path(&self) -> &str {
        &self.path
    }

    /// Return the optional symbol selector.
    pub fn selector(&self) -> Option<&SourceSelector> {
        self.selector.as_ref()
    }
}

impl fmt::Display for SourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.path)?;
        if let Some(selector) = &self.selector {
            write!(formatter, "#{selector}")?;
        }
        Ok(())
    }
}

/// A stable symbol selector.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceSelector {
    /// Exact selector text whose language grammar is not shared here.
    Opaque(String),
    /// A structured Elixir selector from ADR-0119.
    Elixir(ElixirSelector),
}

impl SourceSelector {
    /// Keep selector text without interpreting it.
    pub fn opaque(text: impl Into<String>) -> Self {
        Self::Opaque(text.into())
    }

    /// Create a structured Elixir selector.
    pub fn elixir(selector: ElixirSelector) -> Self {
        Self::Elixir(selector)
    }

    /// Parse one selector, falling back to exact opaque text.
    fn parse(text: &str) -> Self {
        parse_elixir_selector(text)
            .map(Self::Elixir)
            .unwrap_or_else(|| Self::Opaque(text.to_string()))
    }

    /// Return the structured Elixir selector, when this is one.
    pub fn as_elixir(&self) -> Option<&ElixirSelector> {
        match self {
            Self::Elixir(selector) => Some(selector),
            Self::Opaque(_) => None,
        }
    }
}

impl fmt::Display for SourceSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opaque(text) => formatter.write_str(text),
            Self::Elixir(selector) => selector.fmt(formatter),
        }
    }
}

/// One structured Elixir owner or callable selector.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub enum ElixirSelector {
    /// A module or protocol implementation owner.
    Owner(ElixirOwner),
    /// A callable below one owner.
    Callable {
        owner: ElixirOwner,
        kind: ElixirCallableKind,
        name: String,
        arity: usize,
    },
}

impl ElixirSelector {
    /// Create an owner-only selector.
    pub fn owner(owner: ElixirOwner) -> Self {
        Self::Owner(owner)
    }

    /// Create a callable selector.
    pub fn callable(
        owner: ElixirOwner,
        kind: ElixirCallableKind,
        name: impl Into<String>,
        arity: usize,
    ) -> Self {
        Self::Callable {
            owner,
            kind,
            name: name.into(),
            arity,
        }
    }

    /// Return the owner for this selector.
    pub fn owner_identity(&self) -> &ElixirOwner {
        match self {
            Self::Owner(owner) | Self::Callable { owner, .. } => owner,
        }
    }

    /// Return the callable kind when this selector names a callable.
    pub fn callable_kind(&self) -> Option<ElixirCallableKind> {
        match self {
            Self::Owner(_) => None,
            Self::Callable { kind, .. } => Some(*kind),
        }
    }

    /// Return the decoded callable name when this selector names a callable.
    #[cfg(test)]
    fn callable_name(&self) -> Option<&str> {
        match self {
            Self::Owner(_) => None,
            Self::Callable { name, .. } => Some(name),
        }
    }

    /// Return the source arity when this selector names a callable.
    #[cfg(test)]
    fn callable_arity(&self) -> Option<usize> {
        match self {
            Self::Owner(_) => None,
            Self::Callable { arity, .. } => Some(*arity),
        }
    }
}

impl fmt::Display for ElixirSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owner(owner) => owner.fmt(formatter),
            Self::Callable {
                owner,
                kind,
                name,
                arity,
            } => write!(
                formatter,
                "{owner}/{}:{}/{arity}",
                kind.prefix(),
                encode_value(name)
            ),
        }
    }
}

/// The owner part of one Elixir selector.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub enum ElixirOwner {
    /// A normal module-like owner.
    Module { name: String },
    /// A protocol implementation owner.
    Implementation { protocol: String, for_type: String },
}

impl ElixirOwner {
    /// Create a module owner.
    pub fn module(name: impl Into<String>) -> Self {
        Self::Module { name: name.into() }
    }

    /// Create a protocol implementation owner.
    pub fn implementation(protocol: impl Into<String>, for_type: impl Into<String>) -> Self {
        Self::Implementation {
            protocol: protocol.into(),
            for_type: for_type.into(),
        }
    }

    /// Return the module name for a module owner.
    pub fn module_name(&self) -> Option<&str> {
        match self {
            Self::Module { name } => Some(name),
            Self::Implementation { .. } => None,
        }
    }

    /// Return the protocol and type for an implementation owner.
    #[cfg(test)]
    fn implementation_parts(&self) -> Option<(&str, &str)> {
        match self {
            Self::Module { .. } => None,
            Self::Implementation { protocol, for_type } => Some((protocol, for_type)),
        }
    }
}

impl fmt::Display for ElixirOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module { name } => write!(formatter, "module:{}", encode_value(name)),
            Self::Implementation { protocol, for_type } => write!(
                formatter,
                "impl:{}/for:{}",
                encode_value(protocol),
                encode_value(for_type)
            ),
        }
    }
}

/// The callable kind in one canonical Elixir selector.
#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum ElixirCallableKind {
    Function,
    Macro,
    Guard,
    Callback,
    MacroCallback,
}

impl ElixirCallableKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Function => "fn",
            Self::Macro => "macro",
            Self::Guard => "guard",
            Self::Callback => "callback",
            Self::MacroCallback => "macro-callback",
        }
    }

    fn parse(prefix: &str) -> Option<Self> {
        match prefix {
            "fn" => Some(Self::Function),
            "macro" => Some(Self::Macro),
            "guard" => Some(Self::Guard),
            "callback" => Some(Self::Callback),
            "macro-callback" => Some(Self::MacroCallback),
            _ => None,
        }
    }
}

fn parse_elixir_selector(text: &str) -> Option<ElixirSelector> {
    if let Some(rest) = text.strip_prefix("module:") {
        let (name, callable) = split_owner_and_callable(rest);
        let owner = ElixirOwner::module(decode_value(name)?);
        return parse_callable(owner, callable);
    }
    let rest = text.strip_prefix("impl:")?;
    let (protocol, rest) = rest.split_once("/for:")?;
    let (for_type, callable) = split_owner_and_callable(rest);
    let owner = ElixirOwner::implementation(decode_value(protocol)?, decode_value(for_type)?);
    parse_callable(owner, callable)
}

fn split_owner_and_callable(value: &str) -> (&str, Option<&str>) {
    value
        .split_once('/')
        .map_or((value, None), |(owner, callable)| (owner, Some(callable)))
}

fn parse_callable(owner: ElixirOwner, callable: Option<&str>) -> Option<ElixirSelector> {
    let Some(callable) = callable else {
        return Some(ElixirSelector::owner(owner));
    };
    let (kind_and_name, arity_text) = callable.rsplit_once('/')?;
    let (kind, name) = kind_and_name.split_once(':')?;
    if name.contains('/') || arity_text.is_empty() {
        return None;
    }
    let arity = arity_text.parse::<usize>().ok()?;
    if arity.to_string() != arity_text {
        return None;
    }
    Some(ElixirSelector::callable(
        owner,
        ElixirCallableKind::parse(kind)?,
        decode_value(name)?,
        arity,
    ))
}

fn encode_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use fmt::Write as _;
            write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

fn decode_value(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        if !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b'.' | b'_' | b'~') {
            return None;
        }
        decoded.push(byte);
        index += 1;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_every_canonical_elixir_shape() {
        let module = ElixirOwner::module("My.App");
        let implementation = ElixirOwner::implementation("Enumerable", "My.App");
        let cases = [
            (ElixirSelector::owner(module.clone()), "module:My.App"),
            (
                ElixirSelector::callable(module.clone(), ElixirCallableKind::Function, "run", 2),
                "module:My.App/fn:run/2",
            ),
            (
                ElixirSelector::callable(module.clone(), ElixirCallableKind::Macro, "build", 1),
                "module:My.App/macro:build/1",
            ),
            (
                ElixirSelector::callable(module.clone(), ElixirCallableKind::Guard, "valid", 1),
                "module:My.App/guard:valid/1",
            ),
            (
                ElixirSelector::callable(module.clone(), ElixirCallableKind::Callback, "run", 1),
                "module:My.App/callback:run/1",
            ),
            (
                ElixirSelector::callable(module, ElixirCallableKind::MacroCallback, "build", 1),
                "module:My.App/macro-callback:build/1",
            ),
            (
                ElixirSelector::owner(implementation.clone()),
                "impl:Enumerable/for:My.App",
            ),
            (
                ElixirSelector::callable(implementation, ElixirCallableKind::Function, "reduce", 3),
                "impl:Enumerable/for:My.App/fn:reduce/3",
            ),
        ];

        for (selector, expected) in cases {
            let identity =
                SourceIdentity::symbol("lib/sample.ex", SourceSelector::elixir(selector));
            let text = format!("lib/sample.ex#{expected}");
            assert_eq!(identity.to_string(), text);
            assert_eq!(SourceIdentity::parse(&text), identity);
        }
    }

    #[test]
    fn percent_encodes_reserved_and_utf8_bytes() {
        let selector = ElixirSelector::callable(
            ElixirOwner::module("My App.Δ:%/"),
            ElixirCallableKind::Function,
            "+:%/λ",
            2,
        );
        assert_eq!(
            SourceSelector::elixir(selector).to_string(),
            "module:My%20App.%CE%94%3A%25%2F/fn:%2B%3A%25%2F%CE%BB/2"
        );
    }

    #[test]
    fn accepts_lowercase_escapes_and_emits_canonical_uppercase() {
        let identity = SourceIdentity::parse("lib/sample.ex#module:My%20App.%ce%94/fn:%2b/2");
        assert!(identity.selector().unwrap().as_elixir().is_some());
        assert_eq!(
            identity.to_string(),
            "lib/sample.ex#module:My%20App.%CE%94/fn:%2B/2"
        );
    }

    #[test]
    fn keeps_unknown_and_malformed_selectors_opaque() {
        let cases = [
            "fn:run",
            "module:",
            "module:My App",
            "module:My%2",
            "module:My%XZ",
            "module:%FF",
            "module:My.App/fn:run/x",
            "module:My.App/fn:run/01",
            "module:My.App/unknown:run/1",
            "impl:Enumerable",
            "impl:/for:My.App",
            "impl:Enumerable/for:",
            "impl:Enumerable/for:My.App/fn:/1",
        ];

        for selector in cases {
            let identity = SourceIdentity::parse(&format!("lib/sample.ex#{selector}"));
            assert!(
                matches!(identity.selector(), Some(SourceSelector::Opaque(value)) if value == selector),
                "{selector} must stay opaque"
            );
            assert_eq!(identity.to_string(), format!("lib/sample.ex#{selector}"));
        }
    }

    #[test]
    fn preserves_file_and_current_language_text() {
        let file = SourceIdentity::parse("src/lib.rs");
        assert_eq!(file.path(), "src/lib.rs");
        assert!(file.selector().is_none());
        assert_eq!(file.to_string(), "src/lib.rs");

        let symbol = SourceIdentity::parse("src/views.ts#type:A/member:render");
        assert!(matches!(
            symbol.selector(),
            Some(SourceSelector::Opaque(value)) if value == "type:A/member:render"
        ));
        assert_eq!(symbol.to_string(), "src/views.ts#type:A/member:render");
    }

    #[test]
    fn exposes_structured_elixir_parts() {
        let identity =
            SourceIdentity::parse("lib/sample.ex#impl:Enumerable/for:My.App/fn:reduce/3");
        let selector = identity.selector().unwrap().as_elixir().unwrap();
        assert_eq!(
            selector.owner_identity().implementation_parts(),
            Some(("Enumerable", "My.App"))
        );
        assert_eq!(selector.callable_kind(), Some(ElixirCallableKind::Function));
        assert_eq!(selector.callable_name(), Some("reduce"));
        assert_eq!(selector.callable_arity(), Some(3));
    }
}
