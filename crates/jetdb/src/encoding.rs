use crate::format::FormatError;

/// Decode a Latin-1 (ISO 8859-1) byte slice into a `String`.
///
/// Used for Jet3 column names where each byte maps directly to a Unicode
/// code point in the U+0000..U+00FF range.
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Decode a UTF-16LE byte slice into a `String`.
///
/// Used for Jet4/ACE column names. Returns an error if the byte slice has
/// an odd length or contains invalid surrogate pairs.
pub fn decode_utf16le(bytes: &[u8]) -> Result<String, FormatError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(FormatError::InvalidEncoding);
    }
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|_| FormatError::InvalidEncoding)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_ascii() {
        assert_eq!(decode_latin1(b"Hello"), "Hello");
    }

    #[test]
    fn latin1_special_chars() {
        // À = 0xC0, é = 0xE9
        assert_eq!(decode_latin1(&[0xC0, 0xE9]), "Àé");
    }

    #[test]
    fn latin1_empty() {
        assert_eq!(decode_latin1(b""), "");
    }

    #[test]
    fn latin1_full_range() {
        // Every byte 0x00..=0xFF should produce a valid char
        let bytes: Vec<u8> = (0..=255).collect();
        let s = decode_latin1(&bytes);
        assert_eq!(s.chars().count(), 256);
    }

    #[test]
    fn utf16le_ascii() {
        // "Hi" in UTF-16LE
        let bytes = [0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode_utf16le(&bytes).unwrap(), "Hi");
    }

    #[test]
    fn utf16le_japanese() {
        // "日本" = U+65E5 U+672C
        let bytes = [0xE5, 0x65, 0x2C, 0x67];
        assert_eq!(decode_utf16le(&bytes).unwrap(), "日本");
    }

    #[test]
    fn utf16le_empty() {
        assert_eq!(decode_utf16le(&[]).unwrap(), "");
    }

    #[test]
    fn utf16le_odd_length_error() {
        let bytes = [0x48, 0x00, 0x69];
        assert_eq!(decode_utf16le(&bytes), Err(FormatError::InvalidEncoding));
    }

    #[test]
    fn utf16le_invalid_surrogate() {
        // Lone high surrogate: U+D800
        let bytes = [0x00, 0xD8];
        assert_eq!(decode_utf16le(&bytes), Err(FormatError::InvalidEncoding));
    }
}
