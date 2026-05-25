use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SortOptions {
    #[serde(default = "default_true")]
    pub dir_first: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingMode {
    #[default]
    Long,
    Brief,
}
