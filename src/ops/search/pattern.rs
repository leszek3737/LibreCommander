use std::ffi::OsStr;

use memchr::memmem;

/// Unicode-lowercase `s` into a freshly allocated `String` (built directly,
/// no intermediate stack-array encoding).
fn to_lowercase_string(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// Case-insensitive substring test: lowercase `haystack` into `buf` (reused
/// across calls) and run the precomputed `finder` over it. `finder.needle()`
/// must already be lowercased; an empty needle matches everything.
pub(super) fn contains_case_insensitive(
    haystack: &str,
    finder: &memmem::Finder<'_>,
    buf: &mut String,
) -> bool {
    if finder.needle().is_empty() {
        return true;
    }
    buf.clear();
    buf.extend(haystack.chars().flat_map(char::to_lowercase));
    finder.find(buf.as_bytes()).is_some()
}

/// Reusable scratch buffers for [`CompiledPattern::matches_with`].
///
/// Threading one instance through the search hot loop avoids a per-file
/// allocation on the case-insensitive Plain/Wildcard paths. [`CompiledPattern::matches`]
/// allocates a throwaway scratch for one-off callers.
#[derive(Default)]
pub(super) struct MatchScratch {
    /// Lowercased haystack bytes for case-insensitive substring search.
    lower: String,
    /// Lowercased haystack chars for affix / DP wildcard matching.
    chars: Vec<char>,
}

/// A plain (wildcard-free) needle. Single source of truth: `needle` holds the
/// original bytes when case-sensitive, or the lowercased form when not. The
/// `finder` is precomputed once (case-insensitive only) so `matches` never
/// rebuilds it per call.
#[derive(Clone, Debug)]
struct Plain {
    needle: String,
    finder: Option<memmem::Finder<'static>>,
}

impl PartialEq for Plain {
    // `finder` is a precomputed cache of `needle`; equality is determined by
    // the needle alone (and the case-sensitivity flag owned by the enclosing
    // `CompiledPattern`). `memchr::memmem::Finder` does not implement
    // `PartialEq`, so it cannot be derived.
    fn eq(&self, other: &Self) -> bool {
        self.needle == other.needle
    }
}

impl Plain {
    fn new(needle: &str, insensitive: bool) -> Self {
        if insensitive {
            let lowered = to_lowercase_string(needle);
            let finder = memmem::Finder::new(lowered.as_bytes()).into_owned();
            Plain {
                needle: lowered,
                finder: Some(finder),
            }
        } else {
            Plain {
                needle: needle.to_owned(),
                finder: None,
            }
        }
    }

    fn matches(&self, name: &str, scratch: &mut MatchScratch) -> bool {
        match &self.finder {
            Some(finder) => {
                if self.needle.is_empty() {
                    return true;
                }
                // ASCII fast path: fold both sides byte-wise, no allocation.
                if self.needle.is_ascii() && name.is_ascii() {
                    let needle = self.needle.as_bytes();
                    return name
                        .as_bytes()
                        .windows(needle.len())
                        .any(|w| w.eq_ignore_ascii_case(needle));
                }
                contains_case_insensitive(name, finder, &mut scratch.lower)
            }
            None => self.needle.is_empty() || name.contains(self.needle.as_str()),
        }
    }

    /// Match against raw (non-UTF-8) name bytes. Case-insensitive folds ASCII
    /// only — Unicode folding has no meaning over arbitrary bytes.
    #[cfg(unix)]
    fn matches_bytes(&self, name: &[u8], insensitive: bool) -> bool {
        let needle = self.needle.as_bytes();
        if needle.is_empty() {
            return true;
        }
        if insensitive {
            name.windows(needle.len())
                .any(|w| w.eq_ignore_ascii_case(needle))
        } else {
            memmem::find(name, needle).is_some()
        }
    }
}

/// A `prefix*suffix` simple wildcard (the `*inner*` "contains" form is folded
/// into [`Plain`], since a contains-test is exactly a plain substring match).
/// `prefix`/`suffix` are stored lowercased when matching case-insensitively;
/// `None` means that side is empty (a bare `*` is both `None`).
#[derive(Clone, Debug, PartialEq)]
struct WildcardAffix {
    /// Affixes for the case-sensitive (byte) path; for insensitive patterns
    /// these hold the lowercased form and seed `matches_bytes` on non-UTF-8.
    prefix: Option<String>,
    suffix: Option<String>,
    /// Precomputed lowercased chars for the case-insensitive UTF-8 path —
    /// `Some` only for insensitive patterns, so matching avoids re-decoding and
    /// re-counting the affix on every comparison.
    prefix_chars: Option<Box<[char]>>,
    suffix_chars: Option<Box<[char]>>,
}

impl WildcardAffix {
    fn new(pre: &str, suf: &str, insensitive: bool) -> Self {
        let norm = |s: &str| {
            (!s.is_empty()).then(|| {
                if insensitive {
                    to_lowercase_string(s)
                } else {
                    s.to_owned()
                }
            })
        };
        let prefix = norm(pre);
        let suffix = norm(suf);
        // Only insensitive matching consults the char slices; don't pay for them
        // on case-sensitive patterns.
        let to_chars = |s: &Option<String>| {
            insensitive
                .then(|| s.as_ref().map(|x| x.chars().collect::<Box<[char]>>()))
                .flatten()
        };
        let prefix_chars = to_chars(&prefix);
        let suffix_chars = to_chars(&suffix);
        WildcardAffix {
            prefix,
            suffix,
            prefix_chars,
            suffix_chars,
        }
    }

    fn matches(&self, name: &str, insensitive: bool, scratch: &mut MatchScratch) -> bool {
        if !insensitive {
            let prefix_len = self.prefix.as_ref().map_or(0, String::len);
            let suffix_len = self.suffix.as_ref().map_or(0, String::len);
            if name.len() < prefix_len + suffix_len {
                return false;
            }
            return self
                .prefix
                .as_ref()
                .is_none_or(|p| name.starts_with(p.as_str()))
                && self
                    .suffix
                    .as_ref()
                    .is_none_or(|s| name.ends_with(s.as_str()));
        }
        // Case-insensitive: fold the name once into the reused char buffer, then
        // compare the leading/trailing slices against the precomputed (already
        // lowercased) affix slices.
        let chars = &mut scratch.chars;
        chars.clear();
        chars.extend(name.chars().flat_map(char::to_lowercase));
        let prefix_len = self.prefix_chars.as_ref().map_or(0, |p| p.len());
        let suffix_len = self.suffix_chars.as_ref().map_or(0, |s| s.len());
        if chars.len() < prefix_len + suffix_len {
            return false;
        }
        if let Some(prefix) = &self.prefix_chars
            && chars[..prefix_len] != **prefix
        {
            return false;
        }
        if let Some(suffix) = &self.suffix_chars
            && chars[chars.len() - suffix_len..] != **suffix
        {
            return false;
        }
        true
    }

    /// Match against raw (non-UTF-8) name bytes; case-insensitive folds ASCII
    /// only (see [`Plain::matches_bytes`]).
    #[cfg(unix)]
    fn matches_bytes(&self, name: &[u8], insensitive: bool) -> bool {
        let prefix = self.prefix.as_deref().map(str::as_bytes);
        let suffix = self.suffix.as_deref().map(str::as_bytes);
        let prefix_len = prefix.map_or(0, <[u8]>::len);
        let suffix_len = suffix.map_or(0, <[u8]>::len);
        if name.len() < prefix_len + suffix_len {
            return false;
        }
        let prefix_ok = match prefix {
            Some(p) if insensitive => name[..p.len()].eq_ignore_ascii_case(p),
            Some(p) => name.starts_with(p),
            None => true,
        };
        if !prefix_ok {
            return false;
        }
        match suffix {
            Some(s) if insensitive => name[name.len() - s.len()..].eq_ignore_ascii_case(s),
            Some(s) => name.ends_with(s),
            None => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum PatternKind {
    // Boxed: a Plain carries a precomputed memmem::Finder (a Two-Way search
    // table, ~300 bytes), which would otherwise bloat every CompiledPattern.
    Plain(Box<Plain>),
    WildcardAffix(WildcardAffix),
    WildcardDp { pattern: Vec<char> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledPattern {
    kind: PatternKind,
    insensitive: bool,
}

impl CompiledPattern {
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        let insensitive = !case_sensitive;

        if !pattern.contains(['*', '?']) {
            return Self {
                kind: PatternKind::Plain(Box::new(Plain::new(pattern, insensitive))),
                insensitive,
            };
        }

        if let Some(compiled) = Self::try_simple_wildcard(pattern, insensitive) {
            return compiled;
        }

        let pattern_chars = if insensitive {
            pattern.chars().flat_map(char::to_lowercase).collect()
        } else {
            pattern.chars().collect()
        };
        Self {
            kind: PatternKind::WildcardDp {
                pattern: pattern_chars,
            },
            insensitive,
        }
    }

    fn try_simple_wildcard(pattern: &str, insensitive: bool) -> Option<Self> {
        if pattern.contains('?') {
            return None;
        }
        let star_count = pattern.chars().filter(|&c| c == '*').count();
        if star_count == 1 {
            let pos = pattern.find('*')?;
            return Some(Self {
                kind: PatternKind::WildcardAffix(WildcardAffix::new(
                    &pattern[..pos],
                    &pattern[pos + 1..],
                    insensitive,
                )),
                insensitive,
            });
        }
        if star_count == 2 {
            let f = pattern.find('*')?;
            let l = pattern.rfind('*')?;
            if l <= f {
                return None;
            }
            let inner = &pattern[f + 1..l];
            if inner.is_empty() {
                return None;
            }
            // `*inner*` is a pure substring test — represent it as Plain.
            if pattern[..f].is_empty() && pattern[l + 1..].is_empty() {
                return Some(Self {
                    kind: PatternKind::Plain(Box::new(Plain::new(inner, insensitive))),
                    insensitive,
                });
            }
        }
        None
    }

    pub fn matches(&self, name: &str) -> bool {
        let mut scratch = MatchScratch::default();
        self.matches_with(name, &mut scratch)
    }

    /// Like [`matches`](Self::matches) but reuses caller-provided scratch
    /// buffers, avoiding a per-call allocation in the search hot loop.
    pub(super) fn matches_with(&self, name: &str, scratch: &mut MatchScratch) -> bool {
        match &self.kind {
            PatternKind::Plain(plain) => plain.matches(name, scratch),
            PatternKind::WildcardAffix(affix) => affix.matches(name, self.insensitive, scratch),
            PatternKind::WildcardDp { pattern } => {
                let chars = &mut scratch.chars;
                chars.clear();
                if self.insensitive {
                    chars.extend(name.chars().flat_map(char::to_lowercase));
                } else {
                    chars.extend(name.chars());
                }
                Self::greedy_wildcard_match(chars, pattern)
            }
        }
    }

    /// Match an OS file name. Valid-UTF-8 names take the borrowed-`str` fast
    /// path; non-UTF-8 names match on raw bytes (Unix) so `to_string_lossy`'s
    /// `U+FFFD` replacement can never produce a false positive.
    pub(super) fn matches_os(&self, name: &OsStr, scratch: &mut MatchScratch) -> bool {
        match name.to_str() {
            Some(name) => self.matches_with(name, scratch),
            None => self.matches_non_utf8(name, scratch),
        }
    }

    #[cfg(unix)]
    fn matches_non_utf8(&self, name: &OsStr, scratch: &mut MatchScratch) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let bytes = name.as_bytes();
        match &self.kind {
            PatternKind::Plain(plain) => plain.matches_bytes(bytes, self.insensitive),
            PatternKind::WildcardAffix(affix) => affix.matches_bytes(bytes, self.insensitive),
            // The DP matcher needs char boundaries; for this rare combination
            // (non-UTF-8 name + `?`/multi-`*` pattern) fall back to lossy.
            PatternKind::WildcardDp { .. } => self.matches_with(&name.to_string_lossy(), scratch),
        }
    }

    #[cfg(not(unix))]
    fn matches_non_utf8(&self, name: &OsStr, scratch: &mut MatchScratch) -> bool {
        // No portable raw-bytes view of an OsStr; fall back to lossy decoding.
        self.matches_with(&name.to_string_lossy(), scratch)
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

/// Convenience one-shot for tests: compile `pattern` and test `name` against it.
#[cfg(test)]
fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
    let compiled = CompiledPattern::new(pattern, case_sensitive);
    compiled.matches(name)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_file_search_matches_pattern_exact() {
        assert!(matches_pattern("file.txt", "file.txt", true));
        assert!(matches_pattern("file.txt", "file.txt", false));
    }

    #[test]
    fn test_file_search_matches_pattern_plain_contains() {
        assert!(matches_pattern("archive-file.txt", "file", true));
        assert!(!matches_pattern("archive-file.txt", "FILE", true));
        assert!(matches_pattern("archive-file.txt", "FILE", false));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_star() {
        assert!(matches_pattern("file.txt", "*.txt", true));
        assert!(matches_pattern("file.txt", "file.*", true));
        assert!(matches_pattern("file.txt", "*", true));
        assert!(matches_pattern("prefix-foo-suffix", "*foo*", true));
        assert!(matches_pattern("long_file_name.txt", "*.txt", true));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order() {
        assert!(matches_pattern("pre-mid-tail", "pre*mid*", true));
        assert!(matches_pattern("head-mid-suf", "*mid*suf", true));
        assert!(matches_pattern("abXYcdZZ", "ab*cd*", true));
        assert!(matches_pattern("ZZabXYcd", "*ab*cd", true));
        assert!(matches_pattern("preXmidYsuf", "pre*mid*suf", true));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order_false() {
        assert!(!matches_pattern("mid-tail-pre", "pre*mid*", true));
        assert!(!matches_pattern("head-suf-mid", "*mid*suf", true));
        assert!(!matches_pattern("cdXYabZZ", "ab*cd*", true));
        assert!(!matches_pattern("ZZcdXYab", "*ab*cd", true));
        assert!(!matches_pattern("preXsufYmid", "pre*mid*suf", true));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_case_insensitive() {
        assert!(matches_pattern("PRE-MID-tail", "pre*mid*", false));
        assert!(matches_pattern("head-MID-SUF", "*mid*suf", false));
        assert!(matches_pattern("ABxyCDzz", "ab*cd*", false));
        assert!(matches_pattern("zzABxyCD", "*ab*cd", false));
        assert!(matches_pattern("PREfooMIDbarSUF", "pre*mid*suf", false));
        assert!(matches_pattern("prefix-FOO-suffix", "*foo*", false));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_question() {
        assert!(!matches_pattern("file.txt", "file.?", true));
        assert!(matches_pattern("file.txt", "file.???", true));
        assert!(!matches_pattern("file.txt", "file.??", true));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive() {
        assert!(matches_pattern("FILE.TXT", "*.txt", false));
        assert!(matches_pattern("file.txt", "*.TXT", false));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_ascii_substring() {
        assert!(matches_pattern("archive-file.txt", "FILE", false));
        assert!(!matches_pattern("archive-file.txt", "FILE", true));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_unicode_substring() {
        assert!(matches_pattern(
            "istanbul-İSTANBUL.txt",
            "i\u{307}stanbul",
            false
        ));
        assert!(matches_pattern("zażółć.txt", "ŻÓŁĆ", false));
        assert!(!matches_pattern("zażółć.txt", "ŻÓŁĆ", true));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_no_alignment_false_negative() {
        assert!(matches_pattern("aŻółć.txt", "ŻÓŁĆ", false));
    }

    #[test]
    fn wildcard_star_crosses_slash_in_dp() {
        // The DP treats / as a regular char; filenames never contain /,
        // so this is academic but documents the matching behavior.
        assert!(matches_pattern("a/b", "*/b", true));
        assert!(!matches_pattern("a/b/c", "*.txt", true));
    }

    #[test]
    fn wildcard_question_matches_exactly_one_char() {
        assert!(matches_pattern("ab", "a?", true));
        assert!(!matches_pattern("abc", "a?", true));
        assert!(!matches_pattern("a", "a?", true));
        assert!(matches_pattern("a", "?", true));
        assert!(!matches_pattern("", "?", true));
        assert!(matches_pattern("abc", "???", true));
    }

    #[test]
    fn wildcard_mixed_star_and_question() {
        assert!(matches_pattern("file001.txt", "file???.txt", true));
        assert!(!matches_pattern("file1.txt", "file???.txt", true));
        assert!(matches_pattern("file001.txt", "file*.txt", true));
    }

    #[test]
    fn empty_pattern_matches_everything() {
        // Empty plain needle is a substring of every name (including empty).
        assert!(matches_pattern("file.txt", "", true));
        assert!(matches_pattern("file.txt", "", false));
        assert!(matches_pattern("", "", true));
        assert!(matches_pattern("ünïcödé", "", false));
    }

    #[test]
    fn double_star_matches_everything() {
        // `**` is not a simple affix (empty inner) → falls through to the DP
        // matcher, where consecutive stars still match any (incl. empty) name.
        assert!(matches_pattern("file.txt", "**", true));
        assert!(matches_pattern("", "**", true));
        assert!(matches_pattern("anything", "**", false));
        // Triple star behaves the same.
        assert!(matches_pattern("file.txt", "***", true));
    }

    #[test]
    fn star_inner_star_is_substring() {
        // `*foo*` is folded into a plain substring test.
        assert!(matches_pattern("a-foo-b", "*foo*", true));
        assert!(!matches_pattern("a-bar-b", "*foo*", true));
        assert!(matches_pattern("FOObar", "*foo*", false));
        assert!(!matches_pattern("FOObar", "*foo*", true));
    }

    #[test]
    fn very_long_name_plain_and_wildcard() {
        let long = "a".repeat(10_000);
        let mut haystack = long.clone();
        haystack.push_str("needle");
        haystack.push_str(&long);

        assert!(matches_pattern(&haystack, "needle", true));
        assert!(matches_pattern(&haystack, "NEEDLE", false));
        assert!(!matches_pattern(&haystack, "NEEDLE", true));
        assert!(matches_pattern(&haystack, "a*needle*a", true));
        assert!(matches_pattern(&haystack, &format!("{long}*"), true));
        assert!(matches_pattern(&haystack, &format!("*{long}"), true));
        assert!(!matches_pattern(&long, "needle", true));
    }

    #[test]
    fn very_long_name_unicode_insensitive() {
        let pad = "ą".repeat(2_000);
        let name = format!("{pad}ŻÓŁĆ{pad}");
        assert!(matches_pattern(&name, "żółć", false));
        assert!(!matches_pattern(&name, "żółć", true));
    }
}
