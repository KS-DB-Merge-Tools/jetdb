//! Text decoding: Latin-1 (Jet3), UTF-16LE (Jet4+), and Unicode compression.

use crate::format::FormatError;

/// Compressed-text header: `0xFF 0xFE`.
const COMPRESSED_HEADER: [u8; 2] = [0xFF, 0xFE];

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
    if bytes.len() % 2 != 0 {
        return Err(FormatError::InvalidEncoding);
    }
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|_| FormatError::InvalidEncoding)
}

/// Decode a Jet4/ACE text column value.
///
/// Handles both compressed text (prefixed with `0xFF 0xFE`) and raw UTF-16LE.
/// For Jet3 databases, delegates to `decode_latin1` since Jet3 does not use
/// text compression.
pub fn decode_text(data: &[u8], is_jet3: bool) -> Result<String, FormatError> {
    if is_jet3 {
        return Ok(decode_latin1(data));
    }
    if data.is_empty() {
        return Ok(String::new());
    }
    if data.len() >= 2 && data[0] == COMPRESSED_HEADER[0] && data[1] == COMPRESSED_HEADER[1] {
        let expanded = decompress_text(&data[2..]);
        decode_utf16le(&expanded)
    } else {
        decode_utf16le(data)
    }
}

/// Decompress Jet4/ACE compressed text.
///
/// The input is the byte sequence *after* the `0xFF 0xFE` header.
/// Compressed mode expands each byte to `[byte, 0x00]` (Latin-1 → UTF-16LE).
/// A `0x00` byte toggles between compressed and uncompressed modes.
/// In uncompressed mode bytes are copied as-is (pairs of UTF-16LE bytes).
fn decompress_text(data: &[u8]) -> Vec<u8> {
    let mut compressed = true;
    let mut output = Vec::with_capacity(data.len() * 2);
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x00 {
            compressed = !compressed;
            i += 1;
        } else if compressed {
            output.push(b);
            output.push(0x00);
            i += 1;
        } else {
            // Uncompressed mode: copy 2 bytes at a time.
            // If only 1 byte remains, ignore it.
            if i + 1 < data.len() {
                output.push(b);
                output.push(data[i + 1]);
            }
            i += 2;
        }
    }
    output
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

    // -- decompress_text tests ------------------------------------------------

    #[test]
    fn decompress_ascii_only() {
        // "Hello" compressed: each byte → [byte, 0x00]
        let input = b"Hello";
        let result = decompress_text(input);
        assert_eq!(
            result,
            vec![0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F, 0x00]
        );
    }

    #[test]
    fn decompress_empty() {
        assert_eq!(decompress_text(b""), Vec::<u8>::new());
    }

    #[test]
    fn decompress_mixed_segments() {
        // Compressed "AB" → 0x00 → Uncompressed [0xE5, 0x65] → 0x00 → Compressed "C"
        let input = [0x41, 0x42, 0x00, 0xE5, 0x65, 0x00, 0x43];
        let result = decompress_text(&input);
        // A=0x41,0x00  B=0x42,0x00  日=0xE5,0x65  C=0x43,0x00
        assert_eq!(result, vec![0x41, 0x00, 0x42, 0x00, 0xE5, 0x65, 0x43, 0x00]);
    }

    #[test]
    fn decompress_uncompressed_odd_trailing_byte() {
        // Switch to uncompressed, then only 1 byte remains → ignored
        let input = [0x00, 0xAB];
        let result = decompress_text(&input);
        assert_eq!(result, Vec::<u8>::new());
    }

    // -- decode_text tests ----------------------------------------------------

    #[test]
    fn decode_text_compressed_hello() {
        // 0xFF 0xFE header + compressed "Hello"
        let data = [0xFF, 0xFE, 0x48, 0x65, 0x6C, 0x6C, 0x6F];
        assert_eq!(decode_text(&data, false).unwrap(), "Hello");
    }

    #[test]
    fn decode_text_raw_utf16le() {
        // No compression header: raw "Hi" in UTF-16LE
        let data = [0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode_text(&data, false).unwrap(), "Hi");
    }

    #[test]
    fn decode_text_mixed_ascii_japanese() {
        // Compressed "A" → 0x00 → Uncompressed "日" (U+65E5 = 0xE5 0x65) → 0x00 → Compressed "B"
        let mut data = vec![0xFF, 0xFE]; // header
        data.push(0x41); // compressed 'A'
        data.push(0x00); // switch to uncompressed
        data.extend_from_slice(&[0xE5, 0x65]); // 日 in UTF-16LE
        data.push(0x00); // switch to compressed
        data.push(0x42); // compressed 'B'
        assert_eq!(decode_text(&data, false).unwrap(), "A日B");
    }

    #[test]
    fn decode_text_empty() {
        assert_eq!(decode_text(&[], false).unwrap(), "");
    }

    #[test]
    fn decode_text_header_only() {
        let data = [0xFF, 0xFE];
        assert_eq!(decode_text(&data, false).unwrap(), "");
    }

    #[test]
    fn decode_text_jet3_latin1() {
        // Jet3: direct Latin-1 decode, no compression
        let data = [0x48, 0x65, 0x6C, 0x6C, 0x6F];
        assert_eq!(decode_text(&data, true).unwrap(), "Hello");
    }

    #[test]
    fn decode_text_jet3_special_chars() {
        // Jet3: Latin-1 with accented characters
        let data = [0xC0, 0xE9]; // À, é
        assert_eq!(decode_text(&data, true).unwrap(), "Àé");
    }
}
