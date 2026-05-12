// A natural sort implementation in Rust.
// Copyright (c) 2023, sxyazi.
//
// This is a port of the C version of Martin Pool's `strnatcmp.c`:
// http://sourcefrog.net/projects/natsort/
//
// Safe Rust adaptation for LibreCommander.

use std::cmp::Ordering;

macro_rules! return_unless_equal {
    ($ord:expr) => {
        match $ord {
            Ordering::Equal => {}
            ord => return ord,
        }
    };
}

#[inline]
#[allow(dead_code)]
fn compare_left(left: &[u8], right: &[u8], li: &mut usize, ri: &mut usize) -> Ordering {
    loop {
        let lb = left.get(*li).copied();
        let rb = right.get(*ri).copied();

        match (lb, rb) {
            (Some(lb), Some(rb)) if lb.is_ascii_digit() && rb.is_ascii_digit() => {
                return_unless_equal!(lb.cmp(&rb));
            }
            (Some(lb), _) if lb.is_ascii_digit() => return Ordering::Greater,
            (_, Some(rb)) if rb.is_ascii_digit() => return Ordering::Less,
            _ => return Ordering::Equal,
        }

        *li += 1;
        *ri += 1;
    }
}

#[inline]
#[allow(dead_code)]
fn compare_right(left: &[u8], right: &[u8], li: &mut usize, ri: &mut usize) -> Ordering {
    let mut bias = Ordering::Equal;

    loop {
        let lb = left.get(*li).copied();
        let rb = right.get(*ri).copied();

        match (lb, rb) {
            (Some(lb), Some(rb)) if lb.is_ascii_digit() && rb.is_ascii_digit() => {
                if bias == Ordering::Equal {
                    bias = lb.cmp(&rb);
                }
            }
            (Some(lb), _) if lb.is_ascii_digit() => return Ordering::Greater,
            (_, Some(rb)) if rb.is_ascii_digit() => return Ordering::Less,
            _ => return bias,
        }

        *li += 1;
        *ri += 1;
    }
}

#[allow(dead_code)]
pub fn natsort(left: &[u8], right: &[u8], insensitive: bool) -> Ordering {
    let mut li = 0;
    let mut ri = 0;

    let mut l = left.get(li);
    let mut r = right.get(ri);

    loop {
        match (l, r) {
            (Some(&ll), Some(&rr)) => {
                if ll.is_ascii_digit() && rr.is_ascii_digit() {
                    if ll == b'0' || rr == b'0' {
                        return_unless_equal!(compare_left(left, right, &mut li, &mut ri));
                    } else {
                        return_unless_equal!(compare_right(left, right, &mut li, &mut ri));
                    }

                    l = left.get(li);
                    r = right.get(ri);
                    continue;
                }

                if insensitive {
                    return_unless_equal!(ll.to_ascii_lowercase().cmp(&rr.to_ascii_lowercase()));
                } else {
                    return_unless_equal!(ll.cmp(&rr));
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }

        li += 1;
        l = left.get(li);
        ri += 1;
        r = right.get(ri);
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NatKeySegment {
    Text(Vec<u8>),
    Num(Vec<u8>),
}

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
                let has_leading_zero = a.first() == Some(&b'0') || b.first() == Some(&b'0');
                if has_leading_zero {
                    a.cmp(b)
                } else {
                    let sa = strip_leading_zeros(a);
                    let sb = strip_leading_zeros(b);
                    sa.len().cmp(&sb.len()).then(sa.cmp(sb))
                }
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

pub fn natsort_key(name: &[u8], insensitive: bool) -> Vec<NatKeySegment> {
    let mut segments = Vec::new();
    let mut i = 0;

    while i < name.len() {
        if name[i].is_ascii_digit() {
            let start = i;
            while i < name.len() && name[i].is_ascii_digit() {
                i += 1;
            }
            segments.push(NatKeySegment::Num(name[start..i].to_vec()));
        } else {
            let start = i;
            while i < name.len() && !name[i].is_ascii_digit() {
                i += 1;
            }
            let mut text = name[start..i].to_vec();
            if insensitive {
                text.make_ascii_lowercase();
            }
            segments.push(NatKeySegment::Text(text));
        }
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(left: &[&str]) {
        let mut right = left.to_vec();
        right.sort_by(|a, b| natsort(a.as_bytes(), b.as_bytes(), true));
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
        let words = [
            "1-02",
            "1-2",
            "1-20",
            "10-20",
            "fred",
            "jane",
            "pic   7",
            "pic 4 else",
            "pic 5",
            "pic 5 ",
            "pic 5 something",
            "pic 6",
            "pic01",
            "pic02",
            "pic02a",
            "pic02000",
            "pic05",
            "pic2",
            "pic3",
            "pic4",
            "pic100",
            "pic100a",
            "pic120",
            "pic121",
            "tom",
            "x2-g8",
            "x2-y08",
            "x2-y7",
            "x8-y8",
        ];

        sorted(&dates);
        sorted(&fractions);
        sorted(&words);
    }

    #[test]
    fn test_natural_ascending() {
        let items = ["a1.txt", "a2.txt", "a10.txt"];
        sorted(&items);
    }

    #[test]
    fn test_leading_zeros() {
        let key_short = natsort_key(b"pic2", true);
        let key_long = natsort_key(b"pic02", true);
        let key_longer = natsort_key(b"pic02000", true);
        assert_eq!(key_short.cmp(&key_long), Ordering::Greater);
        assert_eq!(key_long.cmp(&key_short), Ordering::Less);
        assert!(key_short > key_longer);
        assert!(key_long < key_longer);
    }

    #[test]
    fn test_natsort_key_leading_zeros() {
        let key_short = natsort_key(b"pic2", false);
        let key_long = natsort_key(b"pic02", false);
        assert_eq!(key_short.cmp(&key_long), Ordering::Greater);
        assert_eq!(key_long.cmp(&key_short), Ordering::Less);
    }

    #[test]
    fn test_empty_and_whitespace() {
        let items = ["", "", " ", "  ", "a"];
        let mut sorted_items = items.to_vec();
        sorted_items.sort_by(|a, b| natsort(a.as_bytes(), b.as_bytes(), true));
        assert_eq!(sorted_items, items);

        assert_eq!(natsort(b"", b"", true), std::cmp::Ordering::Equal);
        assert_eq!(natsort(b" ", b"", true), std::cmp::Ordering::Greater);
        assert_eq!(natsort(b"", b" ", true), std::cmp::Ordering::Less);
        assert_eq!(natsort(b"  ", b" ", true), std::cmp::Ordering::Greater);
        assert_eq!(
            natsort(b"file 1.txt", b"file1.txt", true),
            std::cmp::Ordering::Less
        );
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
            let forward = natsort(a, b, true);
            let reverse = natsort(b, a, true);
            assert_eq!(
                forward,
                reverse.reverse(),
                "antisymmetry failed: natsort({:?}, {:?}) = {:?}, natsort({:?}, {:?}) = {:?}",
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b),
                forward,
                String::from_utf8_lossy(b),
                String::from_utf8_lossy(a),
                reverse,
            );
        }
    }

    #[test]
    fn test_natsort_key_matches_natsort_file_sequence() {
        let names = [
            "file1.txt",
            "file10.txt",
            "file2.txt",
            "file20.txt",
            "file3.txt",
        ];
        let mut by_natsort = names.to_vec();
        by_natsort.sort_by(|a, b| natsort(a.as_bytes(), b.as_bytes(), true));

        let mut by_key = names.to_vec();
        by_key.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));

        assert_eq!(
            by_natsort, by_key,
            "natsort_key must produce same order as natsort"
        );
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
    fn test_natsort_key_pairwise_consistency() {
        let pairs: &[(&[u8], &[u8])] = &[
            (b"file2", b"file10"),
            (b"a2b", b"a10b"),
            (b"z99", b"z100"),
            (b"v2rc14", b"v2rc2"),
            (b"1", b"10"),
            (b"abc", b"abcd"),
        ];
        for (a, b) in pairs {
            let direct = natsort(a, b, true);
            let via_key = natsort_key(a, true).cmp(&natsort_key(b, true));
            assert_eq!(
                direct,
                via_key,
                "natsort vs natsort_key mismatch for {:?} vs {:?}: direct={:?}, key={:?}",
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b),
                direct,
                via_key,
            );
        }
    }

    #[test]
    fn test_natsort_key_mixed_alpha_numeric() {
        let names = ["a10b", "a2b", "a1b"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted, ["a1b", "a2b", "a10b"]);
    }

    #[test]
    fn test_natsort_polish_diacritics() {
        let names = ["ząb", "zab", "źreb", "aąb"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0], "aąb");
        assert_eq!(sorted[1], "zab");
        assert_eq!(sorted[2], "ząb");
        assert_eq!(sorted[3], "źreb");
    }

    #[test]
    fn test_natsort_emoji_filenames() {
        let names = ["📄doc", "📊data", "a📝", "plain"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0], "a📝");
        assert_eq!(sorted[1], "plain");
    }

    #[test]
    fn test_natsort_zero_width_joiner() {
        let zwj_names = ["a\u{200d}b", "a\u{200c}b", "ab", "a\u{200b}b"];
        let mut sorted = zwj_names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0], "ab");
    }

    #[test]
    fn test_natsort_unicode_combining_chars() {
        let names = ["z\u{0301}", "za", "\u{007a}\u{0301}b", "ab"];
        let mut sorted = names.to_vec();
        sorted.sort_by_cached_key(|s| natsort_key(s.as_bytes(), true));
        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0], "ab");
    }

    #[test]
    fn test_natsort_sensitive_ascii() {
        assert_eq!(natsort(b"abc", b"abc", false), Ordering::Equal);
        assert_eq!(natsort(b"abc", b"abd", false), Ordering::Less);
        assert_eq!(natsort(b"abc", b"ABC", false), Ordering::Greater);
    }

    #[test]
    fn test_natsort_sensitive_digits() {
        assert_eq!(natsort(b"a2", b"a10", false), Ordering::Less);
        assert_eq!(natsort(b"a10", b"a2", false), Ordering::Greater);
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
        assert!(NatKeySegment::Text(b"a".to_vec()) < NatKeySegment::Num(b"1".to_vec()));
        assert!(NatKeySegment::Num(b"1".to_vec()) > NatKeySegment::Text(b"a".to_vec()));
    }

    #[test]
    fn test_natsort_key_matches_natsort_leading_zeros() {
        let pairs: &[(&[u8], &[u8])] = &[
            (b"pic2", b"pic02"),
            (b"pic02", b"pic02000"),
            (b"pic2", b"pic02000"),
            (b"00", b"0"),
            (b"001", b"01"),
            (b"pic01", b"pic02"),
            (b"pic05", b"pic2"),
            (b"pic02000", b"pic05"),
            (b"1-02", b"1-2"),
            (b"x2-y08", b"x2-y7"),
        ];
        for (a, b) in pairs {
            let direct = natsort(a, b, true);
            let via_key = natsort_key(a, true).cmp(&natsort_key(b, true));
            assert_eq!(
                direct,
                via_key,
                "natsort vs natsort_key mismatch for {:?} vs {:?}: direct={:?}, key={:?}",
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b),
                direct,
                via_key,
            );
        }
    }
}
