use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn default_true() -> bool {
    true
}

/// Error returned when a [`SortField`], [`Direction`], or [`SortMode`] cannot be
/// parsed from its string form. Carries the offending input so callers (and the
/// serde `Deserialize` impls) can surface a precise message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseSortError {
    /// String did not match any known sort field (canonical name or alias).
    Field(String),
    /// String did not match any known direction.
    Direction(String),
    /// String was not a well-formed `field_direction` sort-mode token.
    Mode(String),
}

impl fmt::Display for ParseSortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(s) => write!(f, "unknown sort field: {s}"),
            Self::Direction(s) => write!(f, "unknown direction: {s}"),
            Self::Mode(s) => write!(f, "invalid sort mode: {s}"),
        }
    }
}

impl std::error::Error for ParseSortError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SortOptions {
    #[serde(default = "default_true")]
    pub dir_first: bool,
    // Canonical key for the case-sensitivity flag is "sensitive"; "sort_sensitive"
    // is the legacy alias kept for old on-disk configs. `config.rs`'s parallel
    // `PersistedSetup` field MUST match this (canonical "sensitive", alias
    // "sort_sensitive") so a config written by either type is read by both.
    #[serde(default, alias = "sort_sensitive")]
    pub sensitive: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        Self {
            dir_first: true,
            sensitive: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortField {
    #[default]
    Name,
    Extension,
    Size,
    ModTime,
    NaturalName,
    Btime,
}

impl SortField {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Extension => "extension",
            Self::Size => "size",
            Self::ModTime => "mod_time",
            Self::NaturalName => "natural_name",
            Self::Btime => "btime",
        }
    }
}

impl FromStr for SortField {
    type Err = ParseSortError;

    /// Parses a sort field from its canonical name (the value produced by
    /// [`SortField::as_str`]). Two legacy aliases are accepted for
    /// backward-compatible configs: `"mod"` for `mod_time` and `"natural"` for
    /// `natural_name`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "name" => Ok(Self::Name),
            "extension" => Ok(Self::Extension),
            "size" => Ok(Self::Size),
            "mod_time" | "mod" => Ok(Self::ModTime),
            "natural_name" | "natural" => Ok(Self::NaturalName),
            "btime" => Ok(Self::Btime),
            other => Err(ParseSortError::Field(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

impl Direction {
    #[must_use]
    pub fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    #[must_use]
    pub fn is_ascending(self) -> bool {
        matches!(self, Self::Asc)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

impl FromStr for Direction {
    type Err = ParseSortError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            other => Err(ParseSortError::Direction(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SortMode {
    pub field: SortField,
    pub direction: Direction,
}

impl SortMode {
    #[must_use]
    pub fn new(field: SortField, direction: Direction) -> Self {
        Self { field, direction }
    }

    #[must_use]
    pub fn is_ascending(self) -> bool {
        self.direction.is_ascending()
    }

    #[must_use]
    pub fn is_descending(self) -> bool {
        !self.is_ascending()
    }

    #[must_use]
    pub fn toggle_direction(self) -> Self {
        Self {
            field: self.field,
            direction: self.direction.flip(),
        }
    }
}

impl fmt::Display for SortMode {
    /// Encodes a sort mode as a single `field_direction` token, e.g. `name_asc`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", self.field.as_str(), self.direction.as_str())
    }
}

impl FromStr for SortMode {
    type Err = ParseSortError;

    /// Parses the `field_direction` token produced by [`SortMode`]'s `Display`.
    /// The direction is the final `_`-delimited segment, so multi-word field
    /// names such as `mod_time` and `natural_name` round-trip correctly.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (field_str, dir_str) = s
            .rsplit_once('_')
            .ok_or_else(|| ParseSortError::Mode(s.to_owned()))?;
        Ok(Self {
            field: field_str.parse()?,
            direction: dir_str.parse()?,
        })
    }
}

// Persisted as the single `field_direction` token rather than a nested table so
// configs stay terse and human-editable; both directions reuse the FromStr /
// Display logic above as the single source of truth.
impl Serialize for SortMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SortMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingMode {
    #[default]
    Long,
    Brief,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn sort_field_canonical_round_trips() {
        // Every variant must parse back from the exact value as_str emits.
        for field in [
            SortField::Name,
            SortField::Extension,
            SortField::Size,
            SortField::ModTime,
            SortField::NaturalName,
            SortField::Btime,
        ] {
            assert_eq!(field.as_str().parse::<SortField>(), Ok(field));
        }
    }

    #[test]
    fn sort_field_accepts_legacy_aliases() {
        // Backward-compat: short aliases from old configs must still parse.
        assert_eq!("mod".parse::<SortField>(), Ok(SortField::ModTime));
        assert_eq!("natural".parse::<SortField>(), Ok(SortField::NaturalName));
        // Canonical names keep working alongside the aliases.
        assert_eq!("mod_time".parse::<SortField>(), Ok(SortField::ModTime));
        assert_eq!(
            "natural_name".parse::<SortField>(),
            Ok(SortField::NaturalName)
        );
    }

    #[test]
    fn sort_field_rejects_unknown() {
        assert_eq!(
            "bogus".parse::<SortField>(),
            Err(ParseSortError::Field("bogus".to_owned()))
        );
    }

    #[test]
    fn direction_round_trips() {
        assert_eq!("asc".parse::<Direction>(), Ok(Direction::Asc));
        assert_eq!("desc".parse::<Direction>(), Ok(Direction::Desc));
        assert_eq!(
            "up".parse::<Direction>(),
            Err(ParseSortError::Direction("up".to_owned()))
        );
    }

    #[test]
    fn sort_mode_from_str_handles_multiword_and_alias_fields() {
        // rsplit_once keeps multi-word canonical field names intact.
        assert_eq!(
            "mod_time_asc".parse::<SortMode>(),
            Ok(SortMode::new(SortField::ModTime, Direction::Asc))
        );
        assert_eq!(
            "natural_name_desc".parse::<SortMode>(),
            Ok(SortMode::new(SortField::NaturalName, Direction::Desc))
        );
        // Alias field embedded in a sort-mode token still resolves.
        assert_eq!(
            "mod_desc".parse::<SortMode>(),
            Ok(SortMode::new(SortField::ModTime, Direction::Desc))
        );
    }

    #[test]
    fn sort_mode_from_str_rejects_malformed() {
        assert_eq!(
            "name".parse::<SortMode>(),
            Err(ParseSortError::Mode("name".to_owned()))
        );
    }

    #[test]
    fn sort_mode_serde_round_trip() {
        // Exercises the custom Serialize -> "name_asc" -> custom Deserialize path
        // through a real serde data format (toml), validating the canonical token.
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
        struct Holder {
            mode: SortMode,
        }

        let original = Holder {
            mode: SortMode::new(SortField::Name, Direction::Asc),
        };
        let text = toml::to_string(&original).expect("serialize sort mode");
        assert!(text.contains("\"name_asc\""), "unexpected toml: {text}");

        let restored: Holder = toml::from_str(&text).expect("deserialize sort mode");
        assert_eq!(restored, original);
    }

    #[test]
    fn sort_mode_serde_round_trip_all_variants() {
        // Canonical-name fields (mod_time, natural_name) must survive a serde
        // round-trip without being mangled by the direction split.
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
        struct Holder {
            mode: SortMode,
        }

        for field in [
            SortField::Name,
            SortField::Extension,
            SortField::Size,
            SortField::ModTime,
            SortField::NaturalName,
            SortField::Btime,
        ] {
            for direction in [Direction::Asc, Direction::Desc] {
                let original = Holder {
                    mode: SortMode::new(field, direction),
                };
                let text = toml::to_string(&original).expect("serialize");
                let restored: Holder = toml::from_str(&text).expect("deserialize");
                assert_eq!(restored, original);
            }
        }
    }
}
