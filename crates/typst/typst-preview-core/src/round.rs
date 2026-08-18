//! Coordinate rounding — P4-11.
//!
//! Two decimal places is 1/100 of a typographic point, about 1/7200 inch —
//! orders of magnitude below anything a screen can show, and below what a
//! 2400 dpi imagesetter could resolve. So this is free visually, and it is the
//! cheaper of the two escape hatches decision 0006 lists: one pass over a
//! string, no protocol change, and none of PNG mode's cost in zoom fidelity or
//! find-in-preview.
//!
//! # It is worth much less than the RFC expected
//!
//! P4-11 was written on the premise that page bytes are dominated by `<use>`
//! elements "at full float precision". The first half is right; the second is
//! not. `typst-svg` already rounds to 9 decimal places and formats through
//! `ryu`, which emits the shortest representation that round-trips — so a
//! coordinate is usually written `12.5`, not `12.500000000001`
//! ([`typst-svg/src/write.rs:104`](https://docs.rs/typst-svg/0.15.1/src/typst_svg/write.rs.html)).
//!
//! Measured on a real `#lorem`-heavy page, this saves **2.9%** (170 KB → 165 KB),
//! not the substantial cut the RFC assumed. Kept because it is still free and
//! still correct, but it is not a lever: if page size ever becomes the binding
//! constraint, PNG mode is the one that moves it. See `research/transport.md`.

/// Round every decimal number in an SVG document to `decimals` places.
///
/// Only touches literals that have *more* precision than asked for, so a
/// coordinate already written as `12.5` is left exactly as it is. Integers,
/// identifiers, and hex colours have no decimal point and are never considered.
pub fn round_coordinates(svg: &str, decimals: usize) -> String {
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(svg.len());
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];

        // A number starts at a digit that is not part of a longer identifier.
        if byte.is_ascii_digit() && !continues_identifier(bytes, index) {
            let (text, next) = scan_number(bytes, index);
            out.push_str(&round_one(text, decimals));
            index = next;
            continue;
        }

        out.push(byte as char);
        index += 1;
    }

    out
}

/// Whether the byte before `index` would make this digit part of a name like
/// `g12` or `path3`, rather than the start of a number.
fn continues_identifier(bytes: &[u8], index: usize) -> bool {
    index > 0 && {
        let previous = bytes[index - 1];
        previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'-'
    }
}

/// Read a `digits[.digits]` run starting at `index`.
fn scan_number(bytes: &[u8], index: usize) -> (&str, usize) {
    let mut end = index;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        let mut fraction = end + 1;
        while fraction < bytes.len() && bytes[fraction].is_ascii_digit() {
            fraction += 1;
        }
        // A trailing `.` with no digits after it is not a decimal number.
        if fraction > end + 1 {
            end = fraction;
        }
    }

    // SAFETY-adjacent: the scan only ever advances over ASCII digits and `.`,
    // so the slice is always on a character boundary.
    (std::str::from_utf8(&bytes[index..end]).unwrap_or_default(), end)
}

/// Shorten one number, if it is longer than asked for.
fn round_one(text: &str, decimals: usize) -> String {
    let Some(dot) = text.find('.') else { return text.to_string() };
    if text.len() - dot - 1 <= decimals {
        return text.to_string();
    }

    let Ok(value) = text.parse::<f64>() else { return text.to_string() };
    let rounded = format!("{value:.decimals$}");

    // `12.50` reads no better than `12.5`, and the trailing zeros are bytes.
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_coordinates_are_shortened() {
        assert_eq!(
            round_coordinates(r#"<use x="123.45678901" y="0.000123"/>"#, 2),
            r#"<use x="123.46" y="0"/>"#
        );
    }

    #[test]
    fn short_coordinates_are_left_alone() {
        let svg = r#"<use x="12.5" y="3" width="100"/>"#;
        assert_eq!(round_coordinates(svg, 2), svg);
    }

    #[test]
    fn identifiers_containing_digits_are_untouched() {
        let svg = r##"<use href="#g12" class="p3-4"/><path id="glyph1.5x"/>"##;
        assert_eq!(round_coordinates(svg, 2), svg);
    }

    #[test]
    fn hex_colours_survive() {
        let svg = r##"<path fill="#1a2b3c" stroke="#000000"/>"##;
        assert_eq!(round_coordinates(svg, 2), svg);
    }

    #[test]
    fn path_data_is_rounded_too() {
        assert_eq!(
            round_coordinates("<path d=\"M 1.23456 2.98765 L 3.5 4\"/>", 2),
            "<path d=\"M 1.23 2.99 L 3.5 4\"/>"
        );
    }

    #[test]
    fn a_version_attribute_is_not_mangled_into_nonsense() {
        // `1.1` already has one decimal place, so nothing happens to it.
        let svg = r#"<svg version="1.1" viewBox="0 0 595.2755 841.8897"/>"#;
        assert_eq!(
            round_coordinates(svg, 2),
            r#"<svg version="1.1" viewBox="0 0 595.28 841.89"/>"#
        );
    }

    #[test]
    fn base64_data_uris_survive() {
        let svg = r#"<image href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUg=="/>"#;
        assert_eq!(round_coordinates(svg, 2), svg);
    }

    #[test]
    fn rounding_never_grows_the_document() {
        let svg = r#"<use x="1.234567" y="99.999999"/><use x="5" y="10.5"/>"#;
        let rounded = round_coordinates(svg, 2);
        assert!(rounded.len() <= svg.len(), "{rounded}");
    }

    #[test]
    fn rounding_up_across_a_carry_is_correct() {
        assert_eq!(round_coordinates(r#"x="9.999"#, 2), r#"x="10"#);
        assert_eq!(round_coordinates(r#"x="0.005"#, 2), r#"x="0.01"#);
    }
}
