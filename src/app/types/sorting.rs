use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    NameAsc,
    NameDesc,
    ExtensionAsc,
    ExtensionDesc,
    SizeAsc,
    SizeDesc,
    ModTimeAsc,
    ModTimeDesc,
    NaturalNameAsc,
    NaturalNameDesc,
    BtimeAsc,
    BtimeDesc,
}

impl SortMode {
    // TODO: After refactoring to SortField + Direction, this becomes a single field comparison
    #[must_use]
    pub fn is_ascending(self) -> bool {
        matches!(
            self,
            Self::NameAsc
                | Self::ExtensionAsc
                | Self::SizeAsc
                | Self::ModTimeAsc
                | Self::NaturalNameAsc
                | Self::BtimeAsc
        )
    }

    #[must_use]
    pub fn is_descending(self) -> bool {
        !self.is_ascending()
    }

    // TODO: Refactor into SortField + Direction enum; toggle becomes flip direction field,
    // eliminates 12 match arms and risk of missing a variant when adding new sort fields.
    #[must_use]
    pub fn toggle_direction(self) -> Self {
        match self {
            Self::NameAsc => Self::NameDesc,
            Self::NameDesc => Self::NameAsc,
            Self::ExtensionAsc => Self::ExtensionDesc,
            Self::ExtensionDesc => Self::ExtensionAsc,
            Self::SizeAsc => Self::SizeDesc,
            Self::SizeDesc => Self::SizeAsc,
            Self::ModTimeAsc => Self::ModTimeDesc,
            Self::ModTimeDesc => Self::ModTimeAsc,
            Self::NaturalNameAsc => Self::NaturalNameDesc,
            Self::NaturalNameDesc => Self::NaturalNameAsc,
            Self::BtimeAsc => Self::BtimeDesc,
            Self::BtimeDesc => Self::BtimeAsc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingMode {
    #[default]
    Long,
    Brief,
}
