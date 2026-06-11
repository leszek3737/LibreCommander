pub(crate) const HEX_OFFSET_PREFIX_WIDTH: usize = 18;
pub(crate) const HEX_BYTES_PER_LINE: usize = 16;
pub(crate) const HEX_PART_WIDTH: usize = HEX_BYTES_PER_LINE * 3 + 1;
pub(crate) const HEX_LINE_WIDTH: usize =
    HEX_OFFSET_PREFIX_WIDTH + HEX_PART_WIDTH + 2 + HEX_BYTES_PER_LINE + 1;

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

#[cfg(test)]
#[must_use]
pub(crate) fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut buf = String::with_capacity(128);
    format_hex_line_to_buffer(offset, bytes, &mut buf);
    buf
}

fn format_offset_hex(offset: usize, buf: &mut String) {
    let mut n = offset as u64;
    for _ in 0..16 {
        buf.push(HEX_CHARS[(n >> 60) as usize] as char);
        n <<= 4;
    }
}

pub(crate) fn format_hex_line_to_buffer(offset: usize, bytes: &[u8], buf: &mut String) {
    format_offset_hex(offset, buf);
    buf.push_str(": ");

    for b in &bytes[..bytes.len().min(8)] {
        buf.push(HEX_CHARS[(b >> 4) as usize] as char);
        buf.push(HEX_CHARS[(b & 0x0f) as usize] as char);
        buf.push(' ');
    }
    if bytes.len() > 8 {
        buf.push(' ');
        for b in &bytes[8..] {
            buf.push(HEX_CHARS[(b >> 4) as usize] as char);
            buf.push(HEX_CHARS[(b & 0x0f) as usize] as char);
            buf.push(' ');
        }
    }

    let hex_expected = bytes.len() * 3 + usize::from(bytes.len() > 8);
    let padding_needed = HEX_PART_WIDTH.saturating_sub(hex_expected);
    for _ in 0..padding_needed {
        buf.push(' ');
    }

    buf.push_str(" |");
    for &b in bytes {
        let c = if (32..=126).contains(&b) {
            b as char
        } else {
            '.'
        };
        buf.push(c);
    }
    buf.push('|');
}

pub(crate) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    if needle.len() == 1 {
        return memchr::memchr(needle[0], haystack);
    }
    memchr::memmem::find(haystack, needle)
}

#[cfg(test)]
mod tests_find_bytes {
    use super::find_bytes;

    #[test]
    fn empty_needle() {
        assert_eq!(find_bytes(b"hello", b""), None);
    }

    #[test]
    fn needle_longer_than_haystack() {
        assert_eq!(find_bytes(b"ab", b"abc"), None);
    }

    #[test]
    fn single_byte_found() {
        assert_eq!(find_bytes(b"hello", b"e"), Some(1));
    }

    #[test]
    fn single_byte_not_found() {
        assert_eq!(find_bytes(b"hello", b"x"), None);
    }

    #[test]
    fn multi_byte_found() {
        assert_eq!(find_bytes(b"hello world", b"world"), Some(6));
    }

    #[test]
    fn multi_byte_not_found() {
        assert_eq!(find_bytes(b"hello world", b"xyz"), None);
    }

    #[test]
    fn at_start() {
        assert_eq!(find_bytes(b"hello", b"he"), Some(0));
    }

    #[test]
    fn at_end() {
        assert_eq!(find_bytes(b"hello", b"lo"), Some(3));
    }
}
