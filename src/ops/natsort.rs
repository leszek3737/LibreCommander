// A natural sort implementation in Rust.
// Copyright (c) 2023, sxyazi.
//
// Inspired by Martin Pool's `strnatcmp.c`:
// http://sourcefrog.net/projects/natsort/
//
// This is NOT a literal port of strnatcmp.c. Key deviation: when a text
// segment is compared against a number segment (cross-variant), Text always
// sorts before Num, so letter-leading names come first. strnatcmp.c compares
// raw byte values left-to-right, which places digit-leading names (lower ASCII)
// first. This deviation is intentional for this project's UX.
//
// Safe Rust adaptation for LibreCommander.

use std::cmp::Ordering;

/// Natural-sort key: a name decomposed into alternating text / number segments.
pub type NatKey = Vec<NatKeySegment>;

/// Owned segment bytes (case-folded for insensitive text segments).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SegData(Box<[u8]>);

impl SegData {
    fn from_slice(s: &[u8]) -> Self {
        Self(s.to_vec().into_boxed_slice())
    }

    fn build(s: &[u8], fold_ascii: bool) -> Self {
        let mut owned = s.to_vec();
        if fold_ascii {
            owned.make_ascii_lowercase();
        }
        Self(owned.into_boxed_slice())
    }

    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NatKeySegment {
    Text(SegData),
    Num(SegData),
}

/// Strip leading ASCII `'0'` bytes, returning the significant digits.
///
/// An all-zero input (`"000"`) yields an empty slice, so two all-zero runs
/// (`"0"` vs `"000"`) compare as numerically equal (both represent 0). The
/// original digit bytes serve as a tiebreaker in [`NatKeySegment::cmp`].
fn strip_leading_zeros(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|&d| d != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

impl Ord for NatKeySegment {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (NatKeySegment::Text(a), NatKeySegment::Text(b)) => a.cmp(b),
            (NatKeySegment::Num(a), NatKeySegment::Num(b)) => {
                let as_ = a.as_slice();
                let bs = b.as_slice();
                let sa = strip_leading_zeros(as_);
                let sb = strip_leading_zeros(bs);
                // Compare numeric value first: longer significant digit run
                // wins, ties broken by the significant digits themselves.
                // This correctly orders zero-padded vs non-padded numbers
                // (e.g. "090" vs "50" → 90 > 50) regardless of padding.
                sa.len()
                    .cmp(&sb.len())
                    .then(sa.cmp(sb))
                    // Same numeric value (e.g. "7" vs "007"): use the original
                    // digit string as a deterministic tiebreaker so that
                    // shorter/zero-padded variants get a stable total order.
                    .then(as_.cmp(bs))
            }
            (NatKeySegment::Text(_), NatKeySegment::Num(_)) => Ordering::Less,
            (NatKeySegment::Num(_), NatKeySegment::Text(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NatKeySegment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn natsort_key(name: &[u8], insensitive: bool) -> NatKey {
    let mut segments = NatKey::new();
    let mut i = 0;

    while i < name.len() {
        if name[i].is_ascii_digit() {
            let start = i;
            while i < name.len() && name[i].is_ascii_digit() {
                i += 1;
            }
            segments.push(NatKeySegment::Num(SegData::from_slice(&name[start..i])));
        } else {
            let start = i;
            while i < name.len() && !name[i].is_ascii_digit() {
                i += 1;
            }
            segments.push(NatKeySegment::Text(SegData::build(
                &name[start..i],
                insensitive,
            )));
        }
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(a: &[u8], b: &[u8], insensitive: bool) -> Ordering {
        natsort_key(a, insensitive).cmp(&natsort_key(b, insensitive))
    }

    fn sorted(left: &[&str]) {
        let mut right = left.to_vec();
        right.sort_by(|a, b| cmp(a.as_bytes(), b.as_bytes(), true));
        assert_eq!(left, right);
    }

    #[test]
    fn test_dates_fractions_words() {
        let dates = [
            "1999-3-3",
            "1999-12-25",
            "2000-1-2",
            "2000-1-10",
            "2000-3-23",
        ];
        let fractions = [
            "1.002.01", "1.002.03", "1.002.08", "1.009.02", "1.009.10", "1.009.20", "1.010.12",
            "1.011.02",
        ];
        // Key-based order: text segments include spaces, so "pic" < "pic " and
        // zero-padded "picNN" runs sort before "pic N" spaced forms.
        // Numeric value dominates, so pic2 < pic3 < pic4 < pic05 < pic100 <
        // pic02000 (the old bytewise-leading-zero order was a bug).
        let words = [
            "fred",
            "jane",
            "pic01",
            "pic02",
            "pic02a",
            "pic2",
            "pic3",
            "pic4",
            "pic05",
            "pic100",
            "pic100a",
            "pic120",
            "pic121",
            "pic02000",
            "pic 4 else",
            "pic 5",
            "pic 5 ",
            "pic 5 something",
            "pic 6",
            "pic   7",
            "tom",
            "x2-g8",
            "x2-y7",
            "x2-y08",
            "x8-y8",
            "1-02",
            "1-2",
            "1-20",
            "10-20",
        ];

        sorted(&dates);
        sorted(&fractions);
        sorted(&words);
    }

    #[test]
    fn test_natural_ascending() {
        sorted(&["a1.txt", "a2.txt", "a10.txt"]);
    }

    #[test]
    fn test_leading_zeros() {
        let key_short = natsort_key(b"pic2", true);
        let key_long = natsort_key(b"pic02", true);
        let key_longer = natsort_key(b"pic02000", true);
        // Same numeric value (2): original digit string breaks the tie.
        // "02" < "2" bytewise (leading zero sorts first).
        assert_eq!(key_short.cmp(&key_long), Ordering::Greater);
        assert_eq!(key_long.cmp(&key_short), Ordering::Less);
        // Different numeric values: 2 < 2000, regardless of zero padding.
        assert!(key_short < key_longer);
        assert!(key_long < key_longer);
    }

    #[test]
    fn test_empty_and_whitespace() {
        let items = ["", "", " ", "  ", "a"];
        let mut sorted_items = items.to_vec();
        sorted_items.sort_by(|a, b| cmp(a.as_bytes(), b.as_bytes(), true));
        assert_eq!(sorted_items, items);

        assert_eq!(cmp(b"", b"", true), Ordering::Equal);
        assert_eq!(cmp(b" ", b"", true), Ordering::Greater);
        assert_eq!(cmp(b"", b" ", true), Ordering::Less);
        assert_eq!(cmp(b"  ", b" ", true), Ordering::Greater);
        // Key-based: "file " > "file", so spaced form sorts after glued form.
        assert_eq!(cmp(b"file 1.txt", b"file1.txt", true), Ordering::Greater);
    }

    #[test]
    fn test_antisymmetry() {
        let pairs: &[(&[u8], &[u8])] = &[
            (b"abc", b"def"),
            (b"1", b"2"),
            (b"a1", b"a10"),
            (b"pic02", b"pic2"),
            (b"z9", b"z10"),
            (b"", b"a"),
            (b"hello world", b"helloWorld"),
            (b"100", b"99"),
            (b"02", b"2"),
            (b"0", b"00"),
        ];

        for (a, b) in pairs {
            let forward = cmp(a, b, true);
            let reverse = cmp(b, a, true);
            assert_eq!(forward, reverse.reverse());
        }
    }

    #[test]
    fn test_file_sequence_order() {
        let names = [
            "file1.txt",
            "file10.txt",
            "file2.txt",
            "file20.txt",
            "file3.txt",
        ];
        let mut by_key = names.to_vec();
        by_key.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(
            by_key,
            [
                "file1.txt",
                "file2.txt",
                "file3.txt",
                "file10.txt",
                "file20.txt"
            ]
        );
    }

    #[test]
    fn test_natsort_key_mixed_alpha_numeric() {
        let names = ["a10b", "a2b", "a1b"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted, ["a1b", "a2b", "a10b"]);
    }

    #[test]
    fn test_natsort_sensitive_ascii() {
        assert_eq!(cmp(b"abc", b"abc", false), Ordering::Equal);
        assert_eq!(cmp(b"abc", b"abd", false), Ordering::Less);
        assert_eq!(cmp(b"abc", b"ABC", false), Ordering::Greater);
    }

    #[test]
    fn test_natsort_sensitive_digits() {
        assert_eq!(cmp(b"a2", b"a10", false), Ordering::Less);
        assert_eq!(cmp(b"a10", b"a2", false), Ordering::Greater);
    }

    #[test]
    fn test_strip_leading_zeros() {
        assert_eq!(strip_leading_zeros(b"000"), b"");
        assert_eq!(strip_leading_zeros(b"00123"), b"123");
        assert_eq!(strip_leading_zeros(b"0"), b"");
        assert_eq!(strip_leading_zeros(b"42"), b"42");
        assert_eq!(strip_leading_zeros(b""), b"");
    }

    #[test]
    fn test_nat_key_segment_cross_variant() {
        assert!(
            NatKeySegment::Text(SegData::from_slice(b"a"))
                < NatKeySegment::Num(SegData::from_slice(b"1"))
        );
        assert!(
            NatKeySegment::Num(SegData::from_slice(b"1"))
                > NatKeySegment::Text(SegData::from_slice(b"a"))
        );
    }

    #[test]
    fn test_natsort_key_case_sensitive_no_fold() {
        assert_ne!(natsort_key(b"Banana", false), natsort_key(b"banana", false));
        assert_eq!(natsort_key(b"a1b", true), natsort_key(b"A1B", true));
        assert_ne!(natsort_key(b"a1b", false), natsort_key(b"A1B", false));
    }

    #[test]
    fn test_long_digit_sequences() {
        assert_eq!(cmp(b"file9", b"file10", true), Ordering::Less);
        assert_eq!(cmp(b"file99", b"file100", true), Ordering::Less);
        let files = ["file99", "file100", "file9", "file10"];
        let mut sorted = files.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted, ["file9", "file10", "file99", "file100"]);
    }

    #[test]
    fn test_transitive_ordering() {
        let a = natsort_key(b"file1", true);
        let b = natsort_key(b"file2", true);
        let c = natsort_key(b"file10", true);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    #[test]
    fn natsort_unicode_ascii_fold_only() {
        assert_ne!(
            natsort_key("café".as_bytes(), true),
            natsort_key("CAFÉ".as_bytes(), true),
        );
        assert_eq!(
            natsort_key("cafe".as_bytes(), true),
            natsort_key("CAFE".as_bytes(), true),
        );
    }

    #[test]
    fn test_natsort_polish_diacritics() {
        let names = ["ząb", "zab", "źreb", "aąb"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted, ["aąb", "zab", "ząb", "źreb"]);
    }

    #[test]
    fn test_natsort_emoji_filenames() {
        let names = ["📄doc", "📊data", "a📝", "plain"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted[0], "a📝");
        assert_eq!(sorted[1], "plain");
    }

    #[test]
    fn test_mixed_zero_padded_and_unpadded() {
        // The bug: leading-zero path used pure bytewise cmp so "090" < "50".
        // After fix: numeric value compared first → 50 < 90.
        let names = ["file090.txt", "file50.txt", "file9.txt"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(
            sorted,
            ["file9.txt", "file50.txt", "file090.txt"],
            "numeric value must dominate, not bytewise leading-zero order"
        );
    }

    #[test]
    fn test_leading_digits_natural() {
        // Names starting with digits (no text prefix).
        let names = ["10abc", "2abc", "1abc"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted, ["1abc", "2abc", "10abc"]);
    }

    #[test]
    fn test_leading_zero_tiebreak() {
        // Same numeric value: original digit string is the tiebreaker.
        // "7" > "007" because "7" > "0" bytewise (shorter/unpadded sorts last).
        assert_eq!(cmp(b"file7", b"file007", true), Ordering::Greater);
        assert_eq!(cmp(b"file007", b"file7", true), Ordering::Less);
        assert_eq!(cmp(b"file007", b"file07", true), Ordering::Less);
    }
}
