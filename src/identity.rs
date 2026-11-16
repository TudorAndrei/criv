use std::fmt;

/// The heading slug: lowercase alphanumeric words joined by `-`.
pub fn slug(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// The H2 sections an active ADR holds, in this order.
pub const ADR_SECTIONS: [&str; 3] = ["Context", "Decision", "Consequences"];

const ADR_PREFIX: &str = "ADR-";

/// An ADR id. Published ADRs keep their legacy number; every new ADR takes a
/// date and its title slug, so concurrent branches cannot collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdrId<'a> {
    /// `ADR-NNNN`, in a file named `NNNN-slug.md`.
    Legacy(u32),
    /// `ADR-YYYYMMDD-slug`, in a file named `YYYYMMDD-slug.md`.
    Dated { date: &'a str, slug: &'a str },
}

impl<'a> AdrId<'a> {
    /// Parse an `ADR-NNNN` or `ADR-YYYYMMDD-slug` id.
    pub fn parse(id: &'a str) -> Option<Self> {
        Self::parse_key(id.strip_prefix(ADR_PREFIX)?)
    }

    /// The id a file stem names: `NNNN`, `NNNN-slug`, or `YYYYMMDD-slug`.
    pub fn of_stem(stem: &'a str) -> Option<Self> {
        legacy_stem(stem).map_or_else(
            || Self::parse_key(stem),
            |(number, _)| Some(Self::Legacy(number)),
        )
    }

    /// The id the stem of an ADR path or link target names, with or without
    /// `.md`.
    pub fn of_path(path: &'a str) -> Option<Self> {
        Self::of_stem(file_stem(path))
    }

    fn parse_key(key: &'a str) -> Option<Self> {
        if let Some(number) = legacy_number(key) {
            return Some(Self::Legacy(number));
        }
        let (date, slug) = key.split_once('-')?;
        (is_civil_date(date) && is_dated_slug(slug)).then_some(Self::Dated { date, slug })
    }

    /// Whether `stem` is the file stem this id requires: `NNNN-slug` for a
    /// legacy id and exactly `YYYYMMDD-slug` for a dated one.
    pub fn names_file(self, stem: &str) -> bool {
        match self {
            Self::Legacy(number) => legacy_stem(stem)
                .is_some_and(|(stem_number, slug)| stem_number == number && slug.is_some()),
            Self::Dated { .. } => self.dated_stem().is_some_and(|expected| expected == stem),
        }
    }

    /// The file name this id requires for an ADR titled with `title_slug`.
    pub fn filename(self, title_slug: &str) -> String {
        match self {
            Self::Legacy(number) => format!("{number:04}-{title_slug}.md"),
            Self::Dated { date, slug } => format!("{date}-{slug}.md"),
        }
    }

    fn dated_stem(self) -> Option<String> {
        match self {
            Self::Legacy(_) => None,
            Self::Dated { date, slug } => Some(format!("{date}-{slug}")),
        }
    }
}

impl fmt::Display for AdrId<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Legacy(number) => write!(formatter, "{ADR_PREFIX}{number:04}"),
            Self::Dated { date, slug } => write!(formatter, "{ADR_PREFIX}{date}-{slug}"),
        }
    }
}

pub fn is_adr_id(value: &str) -> bool {
    AdrId::parse(value).is_some()
}

/// Whether the stem of `path` is a legacy `NNNN` or `NNNN-slug` ADR name.
pub fn is_legacy_adr_path(path: &str) -> bool {
    legacy_stem(file_stem(path)).is_some()
}

/// The dated id of an ADR titled `title` on `date` (`YYYYMMDD`), or why
/// there is none.
pub fn dated_adr_id(date: &str, title: &str) -> Result<String, String> {
    if !is_civil_date(date) {
        return Err(format!(
            "date `{date}` is not a calendar date in YYYYMMDD form"
        ));
    }
    let slug = slug(title);
    if slug.is_empty() {
        return Err(format!(
            "title `{title}` has no letters or digits to form a slug"
        ));
    }
    if !is_dated_slug(&slug) {
        return Err(format!(
            "title slug `{slug}` must use only ASCII letters and digits"
        ));
    }
    Ok(AdrId::Dated { date, slug: &slug }.to_string())
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name)
}

/// Split a legacy ADR file stem, `NNNN` or `NNNN-slug`, into its number and
/// slug.
fn legacy_stem(stem: &str) -> Option<(u32, Option<&str>)> {
    let (number, slug) = stem
        .split_once('-')
        .map_or((stem, None), |(number, slug)| (number, Some(slug)));
    Some((legacy_number(number)?, slug))
}

fn legacy_number(digits: &str) -> Option<u32> {
    (digits.len() == 4 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Lowercase ASCII words of letters and digits joined by single `-`.
fn is_dated_slug(slug: &str) -> bool {
    slug.split('-').all(|word| {
        !word.is_empty()
            && word
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// Whether `date` is eight digits naming a day of the proleptic Gregorian
/// calendar.
fn is_civil_date(date: &str) -> bool {
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let field = |range| {
        date.get(range)
            .and_then(|digits: &str| digits.parse::<u32>().ok())
    };
    let (Some(year), Some(month), Some(day)) = (field(0..4), field(4..6), field(6..8)) else {
        return false;
    };
    days_in_month(year, month).is_some_and(|days| (1..=days).contains(&day))
}

const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// The number of days in `month` (1 to 12) of `year`.
const fn days_in_month(year: u32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

/// The `YYYYMMDD` UTC date `days` after 1970-01-01.
pub fn civil_date(mut days: u64) -> String {
    let mut year = 1970;
    loop {
        let length = if is_leap_year(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days = days.saturating_sub(length);
        year = year.saturating_add(1);
    }
    let mut month = 1;
    while let Some(length) = days_in_month(year, month).map(u64::from)
        && days >= length
    {
        days = days.saturating_sub(length);
        month = month.saturating_add(1);
    }
    format!("{year:04}{month:02}{:02}", days.saturating_add(1))
}

/// The full id of an ADR policy pattern: `<ADR id>/local-id`. Surrounding
/// whitespace in the local id is not part of the id.
pub fn pattern_id(adr_id: &str, local_id: &str) -> String {
    format!("{adr_id}/{}", local_id.trim())
}

/// Split a full pattern id at its first `/` into its ADR id and its
/// non-empty local id.
pub fn split_pattern_id(pattern_id: &str) -> Option<(&str, &str)> {
    let (adr_id, local_id) = pattern_id.split_once('/')?;
    let local_id = local_id.trim();
    (is_adr_id(adr_id) && !local_id.is_empty()).then_some((adr_id, local_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_and_dated_ids() {
        assert_eq!(AdrId::parse("ADR-0042"), Some(AdrId::Legacy(42)));
        assert_eq!(
            AdrId::parse("ADR-20261117-use-dated-ids"),
            Some(AdrId::Dated {
                date: "20261117",
                slug: "use-dated-ids"
            })
        );
        assert_eq!(
            AdrId::parse("ADR-20240229-leap-day").map(|id| id.to_string()),
            Some("ADR-20240229-leap-day".to_owned())
        );
        for invalid in [
            "ADR-42",
            "ADR-0042-slug",
            "ADR-20261117",
            "ADR-20261117-",
            "ADR-20261117-Upper",
            "ADR-20261117-double--dash",
            "ADR-20261117-trailing-",
            "ADR-20261301-bad-month",
            "ADR-20261131-bad-day",
            "ADR-20230229-not-leap",
            "ADR-2026111-short-date",
            "ADR-README",
            "20261117-no-prefix",
        ] {
            assert_eq!(AdrId::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn file_stems_name_their_ids() {
        assert_eq!(
            AdrId::of_path("docs/adr/0007-topic.md"),
            Some(AdrId::Legacy(7))
        );
        assert_eq!(AdrId::of_path("0007"), Some(AdrId::Legacy(7)));
        let dated = AdrId::parse("ADR-20261117-topic").unwrap();
        assert_eq!(AdrId::of_path("docs/adr/20261117-topic.md"), Some(dated));
        assert!(dated.names_file("20261117-topic"));
        assert!(!dated.names_file("20261117-other"));
        assert!(!dated.names_file("20261118-topic"));
        assert!(AdrId::Legacy(7).names_file("0007-anything"));
        assert!(!AdrId::Legacy(7).names_file("0007"));
        assert_eq!(dated.filename("ignored"), "20261117-topic.md");
        assert_eq!(AdrId::Legacy(7).filename("topic"), "0007-topic.md");
        assert!(is_legacy_adr_path("docs/adr/0007-topic.md"));
        assert!(!is_legacy_adr_path("docs/adr/20261117-topic.md"));
    }

    #[test]
    fn dated_ids_take_the_title_slug() {
        assert_eq!(
            dated_adr_id("20261117", "Use Dated ADR IDs!").as_deref(),
            Ok("ADR-20261117-use-dated-adr-ids")
        );
        assert!(dated_adr_id("20261117", "!!!").is_err());
        assert!(dated_adr_id("20261117", "Café").is_err());
        assert!(dated_adr_id("20261332", "Topic").is_err());
    }

    #[test]
    fn civil_dates_count_from_the_unix_epoch() {
        assert_eq!(civil_date(0), "19700101");
        assert_eq!(civil_date(59), "19700301");
        assert_eq!(civil_date(11_016), "20000229");
        assert_eq!(civil_date(19_782), "20240229");
        assert_eq!(civil_date(20_774), "20261117");
    }

    #[test]
    fn pattern_ids_split_at_the_first_slash() {
        assert_eq!(
            split_pattern_id("ADR-20261117-foo/local/id"),
            Some(("ADR-20261117-foo", "local/id"))
        );
        assert_eq!(
            split_pattern_id("ADR-0001/ no-print "),
            Some(("ADR-0001", "no-print"))
        );
        assert_eq!(split_pattern_id("ADR-0001/ "), None);
        assert_eq!(split_pattern_id("guide/local"), None);
    }
}
