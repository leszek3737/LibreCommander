use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SortOptions {
    #[serde(default = "default_true")]
    pub dir_first: bool,
    // Backward compat: old configs used "sort_sensitive" before rename to "sensitive"
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

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "name" => Some(Self::Name),
            "extension" => Some(Self::Extension),
            "size" => Some(Self::Size),
            "mod_time" | "mod" => Some(Self::ModTime),
            "natural_name" | "natural" => Some(Self::NaturalName),
            "btime" => Some(Self::Btime),
            _ => None,
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

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
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

impl Serialize for SortMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = format!("{}_{}", self.field.as_str(), self.direction.as_str());
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for SortMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let (field_str, dir_str) = s
            .rsplit_once('_')
            .ok_or_else(|| serde::de::Error::custom(format!("invalid sort mode: {s}")))?;
        let field = SortField::from_str(field_str)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown sort field: {field_str}")))?;
        let direction = Direction::from_str(dir_str)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown direction: {dir_str}")))?;
        Ok(SortMode { field, direction })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingMode {
    #[default]
    Long,
    Brief,
}
