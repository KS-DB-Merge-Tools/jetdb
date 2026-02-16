//! Money and Numeric fixed-point value formatting.

/// Convert an 8-byte Money value (i64 LE, scale 4) to a string.
///
/// The raw value is a fixed-point integer with 4 decimal places.
/// Example: `123456789i64` → `"12345.6789"`.
pub fn money_to_string(bytes: &[u8; 8]) -> String {
    let raw = i64::from_le_bytes(*bytes);
    let negative = raw < 0;
    let abs_val = (raw as i128).unsigned_abs(); // avoid i64::MIN overflow
    let integer = abs_val / 10_000;
    let decimal = abs_val % 10_000;
    if negative {
        format!("-{integer}.{decimal:04}")
    } else {
        format!("{integer}.{decimal:04}")
    }
}

/// Convert a 17-byte Numeric value to a string.
///
/// Layout: `bytes[0]` = sign (0x00 = positive), `bytes[1..17]` = 128-bit
/// unsigned value stored as four 4-byte little-endian groups that together
/// form a big-endian u128.
///
/// `scale` is the number of decimal places (from the column definition).
pub fn numeric_to_string(bytes: &[u8; 17], scale: u8) -> String {
    let negative = bytes[0] != 0x00;

    // Fix byte order: swap each 4-byte group internally, then interpret as BE u128.
    let mut fixed = [0u8; 16];
    for group in 0..4 {
        let src = 1 + group * 4;
        let dst = group * 4;
        fixed[dst] = bytes[src + 3];
        fixed[dst + 1] = bytes[src + 2];
        fixed[dst + 2] = bytes[src + 1];
        fixed[dst + 3] = bytes[src];
    }
    let value = u128::from_be_bytes(fixed);

    if scale == 0 {
        let s = value.to_string();
        if negative && value != 0 {
            format!("-{s}")
        } else {
            s
        }
    } else {
        let divisor = 10u128.pow(scale as u32);
        let integer = value / divisor;
        let decimal = value % divisor;
        let s = format!("{integer}.{decimal:0>width$}", width = scale as usize);
        if negative && value != 0 {
            format!("-{s}")
        } else {
            s
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- money_to_string ------------------------------------------------------

    #[test]
    fn money_zero() {
        assert_eq!(money_to_string(&0i64.to_le_bytes()), "0.0000");
    }

    #[test]
    fn money_one() {
        assert_eq!(money_to_string(&10_000i64.to_le_bytes()), "1.0000");
    }

    #[test]
    fn money_positive() {
        assert_eq!(money_to_string(&123_456_789i64.to_le_bytes()), "12345.6789");
    }

    #[test]
    fn money_negative() {
        assert_eq!(
            money_to_string(&(-123_456_789i64).to_le_bytes()),
            "-12345.6789"
        );
    }

    #[test]
    fn money_fractional_only() {
        assert_eq!(money_to_string(&5i64.to_le_bytes()), "0.0005");
    }

    #[test]
    fn money_max() {
        let s = money_to_string(&i64::MAX.to_le_bytes());
        // i64::MAX = 9223372036854775807 → 922337203685477.5807
        assert_eq!(s, "922337203685477.5807");
    }

    #[test]
    fn money_min() {
        let s = money_to_string(&i64::MIN.to_le_bytes());
        // i64::MIN = -9223372036854775808 → -922337203685477.5808
        assert_eq!(s, "-922337203685477.5808");
    }

    // -- numeric_to_string ----------------------------------------------------

    #[test]
    fn numeric_zero() {
        let bytes = [0u8; 17];
        assert_eq!(numeric_to_string(&bytes, 2), "0.00");
    }

    #[test]
    fn numeric_positive_integer() {
        // Value 42, scale 0
        let mut bytes = [0u8; 17];
        bytes[0] = 0x00; // positive
                         // 42 as u128 BE = ... 0x2A in last group
                         // Last 4-byte group (bytes 13-16) should contain 42 in swapped LE form
                         // BE u128 for 42: 0x00..002A → group3=[0x00,0x00,0x00,0x2A]
                         // After swap, disk bytes: group3=[0x2A,0x00,0x00,0x00]
        bytes[13] = 0x2A;
        bytes[14] = 0x00;
        bytes[15] = 0x00;
        bytes[16] = 0x00;
        assert_eq!(numeric_to_string(&bytes, 0), "42");
    }

    #[test]
    fn numeric_with_scale() {
        // Value 12345 with scale 2 → "123.45"
        let mut bytes = [0u8; 17];
        bytes[0] = 0x00; // positive
                         // 12345 = 0x3039 → BE group3 = [0x00, 0x00, 0x30, 0x39]
                         // Disk (LE per group): [0x39, 0x30, 0x00, 0x00]
        bytes[13] = 0x39;
        bytes[14] = 0x30;
        bytes[15] = 0x00;
        bytes[16] = 0x00;
        assert_eq!(numeric_to_string(&bytes, 2), "123.45");
    }

    #[test]
    fn numeric_negative() {
        // Negative 12345 with scale 2 → "-123.45"
        let mut bytes = [0u8; 17];
        bytes[0] = 0x80; // negative (non-zero)
        bytes[13] = 0x39;
        bytes[14] = 0x30;
        bytes[15] = 0x00;
        bytes[16] = 0x00;
        assert_eq!(numeric_to_string(&bytes, 2), "-123.45");
    }

    #[test]
    fn numeric_negative_zero() {
        // Negative sign but zero value → "0.00" (no minus sign)
        let mut bytes = [0u8; 17];
        bytes[0] = 0x80;
        assert_eq!(numeric_to_string(&bytes, 2), "0.00");
    }

    #[test]
    fn numeric_leading_decimal_zeros() {
        // Value 5 with scale 4 → "0.0005"
        let mut bytes = [0u8; 17];
        bytes[0] = 0x00;
        bytes[13] = 0x05;
        assert_eq!(numeric_to_string(&bytes, 4), "0.0005");
    }

    #[test]
    fn numeric_multi_group() {
        // Value that spans multiple 4-byte groups:
        // 0x0000_0001_0000_0000 = 4294967296
        // BE: [0x00..00 0x01 0x00 0x00 0x00 0x00]
        // group2=[0x00,0x00,0x00,0x01], group3=[0x00,0x00,0x00,0x00]
        // Disk LE: group2=[0x01,0x00,0x00,0x00], group3=[0x00,0x00,0x00,0x00]
        let mut bytes = [0u8; 17];
        bytes[0] = 0x00;
        bytes[9] = 0x01; // group2 byte 0 (LE)
        assert_eq!(numeric_to_string(&bytes, 0), "4294967296");
    }

    #[test]
    fn numeric_negative_scale_zero() {
        // negative=true (sign byte 0x80), value=42, scale=0
        let mut bytes = [0u8; 17];
        bytes[0] = 0x80; // negative
        bytes[13] = 0x2A; // 42 in LE group form
        assert_eq!(numeric_to_string(&bytes, 0), "-42");
    }

    #[test]
    fn numeric_positive_scale_zero() {
        // positive, value=100, scale=0
        let mut bytes = [0u8; 17];
        bytes[0] = 0x00;
        bytes[13] = 0x64; // 100 in LE group form
        assert_eq!(numeric_to_string(&bytes, 0), "100");
    }
}
