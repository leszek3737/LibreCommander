pub(crate) const HEX_OFFSET_PREFIX_WIDTH: usize = 18;
pub(crate) const HEX_BYTES_PER_LINE: usize = 16;
pub(crate) const HEX_PART_WIDTH: usize = HEX_BYTES_PER_LINE * 3 + 1;
pub(crate) const HEX_LINE_WIDTH: usize =
    HEX_OFFSET_PREFIX_WIDTH + HEX_PART_WIDTH + 2 + HEX_BYTES_PER_LINE + 1;

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Bits in a hex nibble; a byte is two nibbles, a `u64` offset is 16.
const NIBBLE_BITS: u32 = 4;
/// Mask selecting the low nibble of a byte.
const LOW_NIBBLE_MASK: u8 = 0x0f;
/// Hex digits needed to render a `u64` offset (`64 / 4`).
const OFFSET_HEX_DIGITS: usize = (u64::BITS / NIBBLE_BITS) as usize;
/// Bytes shown before the extra gap that splits a row into two visual groups.
const HEX_GROUP_SIZE: usize = HEX_BYTES_PER_LINE / 2;
/// Columns a byte occupies in the hex pane: two nibble digits plus a space.
const HEX_COLS_PER_BYTE: usize = 3;
/// Printable ASCII range used by the right-hand text pane.
const PRINTABLE_ASCII: std::ops::RangeInclusive<u8> = 0x20..=0x7e;

#[cfg(test)]
#[must_use]
pub(crate) fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut buf = String::with_capacity(128);
    format_hex_line_to_buffer(offset, bytes, &mut buf);
    buf
}

fn push_byte_hex(buf: &mut String, b: u8) {
    buf.push(HEX_CHARS[(b >> NIBBLE_BITS) as usize] as char);
    buf.push(HEX_CHARS[(b & LOW_NIBBLE_MASK) as usize] as char);
    buf.push(' ');
}

fn format_offset_hex(offset: usize, buf: &mut String) {
    // Render the 16 most-significant-first nibbles into a stack buffer, then
    // append in one shot rather than pushing each digit individually.
    let mut digits = [0u8; OFFSET_HEX_DIGITS];
    let n = offset as u64;
    for (i, slot) in digits.iter_mut().enumerate() {
        let shift = (OFFSET_HEX_DIGITS - 1 - i) as u32 * NIBBLE_BITS;
        *slot = HEX_CHARS[((n >> shift) & u64::from(LOW_NIBBLE_MASK)) as usize];
    }
    // `digits` is built solely from `HEX_CHARS`, so it is always valid ASCII.
    debug_assert!(
        std::str::from_utf8(&digits).is_ok(),
        "HEX_CHARS must be ASCII"
    );
    if let Ok(s) = std::str::from_utf8(&digits) {
        buf.push_str(s);
    }
}

pub(crate) fn format_hex_line_to_buffer(offset: usize, bytes: &[u8], buf: &mut String) {
    format_offset_hex(offset, buf);
    buf.push_str(": ");

    let split = bytes.len().min(HEX_GROUP_SIZE);
    for &b in &bytes[..split] {
        push_byte_hex(buf, b);
    }
    if bytes.len() > HEX_GROUP_SIZE {
        buf.push(' ');
        for &b in &bytes[HEX_GROUP_SIZE..] {
            push_byte_hex(buf, b);
        }
    }

    let hex_expected = bytes.len() * HEX_COLS_PER_BYTE + usize::from(bytes.len() > HEX_GROUP_SIZE);
    let padding_needed = HEX_PART_WIDTH.saturating_sub(hex_expected);
    buf.extend(std::iter::repeat_n(' ', padding_needed));

    buf.push_str(" |");
    for &b in bytes {
        let c = if PRINTABLE_ASCII.contains(&b) {
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
