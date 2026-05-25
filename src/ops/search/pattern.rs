use crate::ops::search::FileSearch;

pub(super) fn contains_case_insensitive(haystack: &str, needle_lower: &[char]) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let lowered: Vec<char> = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
    lowered
        .windows(needle_lower.len())
        .any(|w| w == needle_lower)
}

pub enum CompiledPattern {
    Plain {
        needle: Vec<char>,
        needle_str: String,
        needle_ascii: Option<String>,
        insensitive: bool,
    },
    WildcardSimple {
        prefix: Option<Vec<char>>,
        suffix: Option<Vec<char>>,
        contains: bool,
        insensitive: bool,
    },
    WildcardDp {
        chars: Vec<char>,
        insensitive: bool,
    },
}

impl CompiledPattern {
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        let insensitive = !case_sensitive;

        if !pattern.contains(['*', '?']) {
            let needle: Vec<char> = if insensitive {
                pattern.chars().flat_map(|c| c.to_lowercase()).collect()
            } else {
                pattern.chars().collect()
            };
            let needle_ascii = if insensitive && pattern.is_ascii() {
                Some(pattern.to_ascii_lowercase())
            } else {
                None
            };
            let needle_str: String = needle.iter().collect();
            return Self::Plain {
                needle,
                needle_str,
                needle_ascii,
                insensitive,
            };
        }

        if let Some(simple) = Self::try_simple_wildcard(pattern, insensitive) {
            return simple;
        }

        let chars = if insensitive {
            pattern.chars().flat_map(|c| c.to_lowercase()).collect()
        } else {
            pattern.chars().collect()
        };
        Self::WildcardDp { chars, insensitive }
    }

    fn try_simple_wildcard(pattern: &str, insensitive: bool) -> Option<Self> {
        if pattern.contains('?') {
            return None;
        }
        let star_count = pattern.chars().filter(|&c| c == '*').count();
        if star_count == 1 {
            let pos = pattern.find('*')?;
            let prefix_str = &pattern[..pos];
            let suffix_str = &pattern[pos + 1..];
            let prefix = Self::maybe_lower(prefix_str, insensitive);
            let suffix = Self::maybe_lower(suffix_str, insensitive);
            return Some(Self::WildcardSimple {
                prefix,
                suffix,
                contains: false,
                insensitive,
            });
        }
        if star_count == 2 {
            let f = pattern.find('*')?;
            let l = pattern.rfind('*')?;
            if l <= f {
                return None;
            }
            let prefix_str = &pattern[..f];
            let inner_str = &pattern[f + 1..l];
            let suffix_str = &pattern[l + 1..];
            if inner_str.is_empty() {
                return None;
            }
            let prefix_empty = prefix_str.is_empty();
            let suffix_empty = suffix_str.is_empty();
            if prefix_empty && suffix_empty {
                let inner = Self::maybe_lower(inner_str, insensitive)?;
                return Some(Self::WildcardSimple {
                    prefix: None,
                    suffix: Some(inner),
                    contains: true,
                    insensitive,
                });
            }
        }
        None
    }

    fn maybe_lower(s: &str, insensitive: bool) -> Option<Vec<char>> {
        if s.is_empty() {
            return None;
        }
        Some(if insensitive {
            s.chars().flat_map(|c| c.to_lowercase()).collect()
        } else {
            s.chars().collect()
        })
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Plain {
                needle,
                needle_str: _,
                needle_ascii,
                insensitive: true,
            } => {
                if needle.is_empty() {
                    return true;
                }
                if let Some(ascii_needle) = needle_ascii
                    && name.is_ascii()
                {
                    return name
                        .as_bytes()
                        .windows(ascii_needle.len())
                        .any(|w| w.eq_ignore_ascii_case(ascii_needle.as_bytes()));
                }
                contains_case_insensitive(name, needle)
            }
            Self::Plain {
                needle: _,
                needle_str,
                needle_ascii: _,
                insensitive: false,
            } => {
                if needle_str.is_empty() {
                    return true;
                }
                name.contains(needle_str.as_str())
            }
            Self::WildcardSimple {
                prefix,
                suffix,
                contains,
                insensitive,
            } => {
                let name_chars: Vec<char> = if *insensitive {
                    name.chars().flat_map(|c| c.to_lowercase()).collect()
                } else {
                    name.chars().collect()
                };
                if *contains {
                    return suffix.as_deref().is_some_and(|suffix_chars| {
                        name_chars
                            .windows(suffix_chars.len())
                            .any(|window| window == suffix_chars)
                    });
                }
                let prefix_len = prefix.as_ref().map_or(0, |p: &Vec<char>| p.len());
                let suffix_len = suffix.as_ref().map_or(0, |s: &Vec<char>| s.len());
                if name_chars.len() < prefix_len + suffix_len {
                    return false;
                }
                if let Some(prefix_chars) = prefix {
                    if name_chars.len() < prefix_chars.len() {
                        return false;
                    }
                    if name_chars[..prefix_chars.len()] != prefix_chars[..] {
                        return false;
                    }
                }
                if let Some(suffix_chars) = suffix {
                    if name_chars.len() < suffix_chars.len() {
                        return false;
                    }
                    let start = name_chars.len() - suffix_chars.len();
                    if name_chars[start..] != suffix_chars[..] {
                        return false;
                    }
                }
                true
            }
            // NOTE: to_lowercase() can expand one char to multiple (e.g. İ -> i + \u{307}).
            // The matcher treats each folded char independently, so `?` may partially
            // match a multi-char lowercase expansion. Known limitation for Turkish İ,
            // German ß, and similar. Full fix requires index-map from original positions
            // to folded ranges.
            Self::WildcardDp { chars, insensitive } => {
                let name_chars: Vec<char> = if *insensitive {
                    name.chars().flat_map(|c| c.to_lowercase()).collect()
                } else {
                    name.chars().collect()
                };
                Self::greedy_wildcard_match(&name_chars, chars)
            }
        }
    }

    fn greedy_wildcard_match(name: &[char], pattern: &[char]) -> bool {
        let mut ni = 0;
        let mut pi = 0;
        let mut star_pi: Option<usize> = None;
        let mut star_ni = 0;
        while ni < name.len() {
            match pattern.get(pi) {
                Some('*') => {
                    star_pi = Some(pi);
                    star_ni = ni;
                    pi += 1;
                }
                Some('?') => {
                    ni += 1;
                    pi += 1;
                }
                Some(c) if name[ni] == *c => {
                    ni += 1;
                    pi += 1;
                }
                _ => match star_pi {
                    Some(sp) => {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                    }
                    None => return false,
                },
            }
        }
        while pi < pattern.len() && pattern[pi] == '*' {
            pi += 1;
        }
        pi == pattern.len()
    }
}

impl FileSearch {
    pub fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
        let compiled = CompiledPattern::new(pattern, case_sensitive);
        compiled.matches(name)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_file_search_matches_pattern_exact() {
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", false));
    }

    #[test]
    fn test_file_search_matches_pattern_plain_contains() {
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "file",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            false
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_star() {
        assert!(FileSearch::matches_pattern("file.txt", "*.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.*", true));
        assert!(FileSearch::matches_pattern("file.txt", "*", true));
        assert!(FileSearch::matches_pattern(
            "prefix-foo-suffix",
            "*foo*",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "long_file_name.txt",
            "*.txt",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order() {
        assert!(FileSearch::matches_pattern(
            "pre-mid-tail",
            "pre*mid*",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "head-mid-suf",
            "*mid*suf",
            true
        ));
        assert!(FileSearch::matches_pattern("abXYcdZZ", "ab*cd*", true));
        assert!(FileSearch::matches_pattern("ZZabXYcd", "*ab*cd", true));
        assert!(FileSearch::matches_pattern(
            "preXmidYsuf",
            "pre*mid*suf",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order_false() {
        assert!(!FileSearch::matches_pattern(
            "mid-tail-pre",
            "pre*mid*",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "head-suf-mid",
            "*mid*suf",
            true
        ));
        assert!(!FileSearch::matches_pattern("cdXYabZZ", "ab*cd*", true));
        assert!(!FileSearch::matches_pattern("ZZcdXYab", "*ab*cd", true));
        assert!(!FileSearch::matches_pattern(
            "preXsufYmid",
            "pre*mid*suf",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_case_insensitive() {
        assert!(FileSearch::matches_pattern(
            "PRE-MID-tail",
            "pre*mid*",
            false
        ));
        assert!(FileSearch::matches_pattern(
            "head-MID-SUF",
            "*mid*suf",
            false
        ));
        assert!(FileSearch::matches_pattern("ABxyCDzz", "ab*cd*", false));
        assert!(FileSearch::matches_pattern("zzABxyCD", "*ab*cd", false));
        assert!(FileSearch::matches_pattern(
            "PREfooMIDbarSUF",
            "pre*mid*suf",
            false
        ));
        assert!(FileSearch::matches_pattern(
            "prefix-FOO-suffix",
            "*foo*",
            false
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_question() {
        assert!(!FileSearch::matches_pattern("file.txt", "file.?", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.???", true));
        assert!(!FileSearch::matches_pattern("file.txt", "file.??", true));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive() {
        assert!(FileSearch::matches_pattern("FILE.TXT", "*.txt", false));
        assert!(FileSearch::matches_pattern("file.txt", "*.TXT", false));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_ascii_substring() {
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            false
        ));
        assert!(!FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_unicode_substring() {
        assert!(FileSearch::matches_pattern(
            "istanbul-İSTANBUL.txt",
            "i\u{307}stanbul",
            false
        ));
    }

    #[test]
    fn wildcard_star_crosses_slash_in_dp() {
        // The DP treats / as a regular char; filenames never contain /,
        // so this is academic but documents the matching behavior.
        assert!(FileSearch::matches_pattern("a/b", "*/b", true));
        assert!(!FileSearch::matches_pattern("a/b/c", "*.txt", true));
    }

    #[test]
    fn wildcard_question_matches_exactly_one_char() {
        assert!(FileSearch::matches_pattern("ab", "a?", true));
        assert!(!FileSearch::matches_pattern("abc", "a?", true));
        assert!(!FileSearch::matches_pattern("a", "a?", true));
        assert!(FileSearch::matches_pattern("a", "?", true));
        assert!(!FileSearch::matches_pattern("", "?", true));
        assert!(FileSearch::matches_pattern("abc", "???", true));
    }

    #[test]
    fn wildcard_mixed_star_and_question() {
        assert!(FileSearch::matches_pattern(
            "file001.txt",
            "file???.txt",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "file1.txt",
            "file???.txt",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "file001.txt",
            "file*.txt",
            true
        ));
    }
}
