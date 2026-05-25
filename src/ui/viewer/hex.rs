pub const HEX_OFFSET_PREFIX_WIDTH: usize = 10;
pub const HEX_BYTES_PER_LINE: usize = 16;
pub const HEX_PART_WIDTH: usize = HEX_BYTES_PER_LINE * 3 + 1;
pub const HEX_LINE_WIDTH: usize = 10 + HEX_PART_WIDTH + 2 + HEX_BYTES_PER_LINE + 1;

#[cfg(test)]
#[must_use]
pub(crate) fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut buf = String::with_capacity(128);
    format_hex_line_to_buffer(offset, bytes, &mut buf);
    buf
}

pub(crate) fn format_hex_line_to_buffer(offset: usize, bytes: &[u8], buf: &mut String) {
    use std::fmt::Write;
    let _ = write!(buf, "{offset:08x}: ");

    let hex_start = buf.len();
    for (i, b) in bytes.iter().enumerate() {
        if i == 8 {
            buf.push(' ');
        }
        let _ = write!(buf, "{b:02x} ");
    }

    let padding_needed = HEX_PART_WIDTH.saturating_sub(buf.len() - hex_start);
    let _ = write!(buf, "{:width$}", "", width = padding_needed);

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
