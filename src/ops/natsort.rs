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
    Num { value: u64, raw_len: usize },
}

impl Ord for NatKeySegment {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (NatKeySegment::Text(a), NatKeySegment::Text(b)) => a.cmp(b),
            (
                NatKeySegment::Num {
                    value: va,
                    raw_len: la,
                },
                NatKeySegment::Num {
                    value: vb,
                    raw_len: lb,
                },
            ) => va.cmp(vb).then(la.cmp(lb)),
            (NatKeySegment::Text(_), NatKeySegment::Num { .. }) => Ordering::Less,
            (NatKeySegment::Num { .. }, NatKeySegment::Text(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NatKeySegment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn parse_u64_digits(bytes: &[u8]) -> Option<u64> {
    let mut result: u64 = 0;
    for &b in bytes {
        let digit = u64::from(b - b'0');
        result = result.checked_mul(10)?.checked_add(digit)?;
    }
    Some(result)
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
            let num = parse_u64_digits(&name[start..i]).unwrap_or(u64::MAX);
            segments.push(NatKeySegment::Num {
                value: num,
                raw_len: i - start,
            });
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
        let items = ["pic02", "pic02000", "pic2"];
        sorted(&items);
    }

    #[test]
    fn test_natsort_key_leading_zeros() {
        let key_short = natsort_key(b"pic2", false);
        let key_long = natsort_key(b"pic02", false);
        // Both have value=2, but raw_len differs: 1 vs 2
        // Shorter raw_len should sort first
        assert_eq!(key_short.cmp(&key_long), Ordering::Less);
        assert_eq!(key_long.cmp(&key_short), Ordering::Greater);
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
}
