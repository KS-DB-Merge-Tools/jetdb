/// Timestamp (f64, days since 1899-12-30) → date/time conversion utilities.
///
/// Microsoft Access stores date/time values as IEEE 754 double-precision
/// floating-point numbers representing fractional days since the epoch
/// 1899-12-30 00:00:00.  The integer part is the day count and the
/// fractional part is the time of day.
/// Access epoch expressed as Julian Day Number (1899-12-30).
const ACCESS_EPOCH_JDN: i64 = 2_415_019;

/// Seconds in a day.
const SECS_PER_DAY: f64 = 86_400.0;

/// Convert an Access timestamp to calendar parts.
///
/// Returns `(year, month, day, hour, minute, second)`.
pub fn timestamp_to_parts(ts: f64) -> (i32, u32, u32, u32, u32, u32) {
    if !ts.is_finite() {
        return (1899, 12, 30, 0, 0, 0); // epoch
    }
    let days = ts.floor() as i64;
    let jdn = ACCESS_EPOCH_JDN + days;

    let (year, month, day) = jdn_to_gregorian(jdn);

    let frac = (ts - ts.floor()).abs();
    let total_secs = (frac * SECS_PER_DAY + 0.5) as u32; // round
    let hour = total_secs / 3600;
    let minute = (total_secs % 3600) / 60;
    let second = total_secs % 60;

    (year, month, day, hour, minute, second)
}

/// Format an Access timestamp using a strftime-like format string.
///
/// Supported directives: `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%%`.
pub fn format_timestamp(ts: f64, fmt: &str) -> String {
    let (year, month, day, hour, minute, second) = timestamp_to_parts(ts);
    let mut result = String::with_capacity(fmt.len() + 8);
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{year:04}")),
                Some('m') => result.push_str(&format!("{month:02}")),
                Some('d') => result.push_str(&format!("{day:02}")),
                Some('H') => result.push_str(&format!("{hour:02}")),
                Some('M') => result.push_str(&format!("{minute:02}")),
                Some('S') => result.push_str(&format!("{second:02}")),
                Some('%') => result.push('%'),
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Return `true` if the timestamp has no time component (date only).
///
/// Tolerates floating-point noise up to 1e-9.
pub fn is_date_only(ts: f64) -> bool {
    if !ts.is_finite() {
        return true;
    }
    let frac = (ts - ts.floor()).abs();
    frac < 1e-9
}

/// Convert Julian Day Number to Gregorian (year, month, day).
///
/// Uses the algorithm from Wikipedia "Julian day" § Converting Gregorian
/// calendar date from Julian day number.
fn jdn_to_gregorian(jdn: i64) -> (i32, u32, u32) {
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146_097;
    let c = a - (146_097 * b) / 4;

    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;

    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;

    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero() {
        let (y, m, d, h, min, s) = timestamp_to_parts(0.0);
        assert_eq!((y, m, d, h, min, s), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn known_date_2003_01_02() {
        // 2003-01-02 is day 37623 from 1899-12-30
        let ts = 37623.0;
        let (y, m, d, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2003, 1, 2));
        assert_eq!((h, min, s), (0, 0, 0));
    }

    #[test]
    fn date_with_time() {
        // 0.5 = noon
        let ts = 37623.5;
        let (y, m, d, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2003, 1, 2));
        assert_eq!((h, min, s), (12, 0, 0));
    }

    #[test]
    fn date_with_time_detailed() {
        // 0.75 = 18:00
        let ts = 37623.75;
        let (_, _, _, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((h, min, s), (18, 0, 0));
    }

    #[test]
    fn leap_year_feb29() {
        // 2000-02-29 is day 36585 from 1899-12-30
        let ts = 36585.0;
        let (y, m, d, _, _, _) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2000, 2, 29));
    }

    #[test]
    fn negative_value() {
        // Day -1 = 1899-12-29
        let ts = -1.0;
        let (y, m, d, _, _, _) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (1899, 12, 29));
    }

    #[test]
    fn is_date_only_true() {
        assert!(is_date_only(37623.0));
    }

    #[test]
    fn is_date_only_false() {
        assert!(!is_date_only(37623.5));
    }

    #[test]
    fn is_date_only_epsilon() {
        assert!(is_date_only(37623.0 + 1e-12));
    }

    #[test]
    fn format_year() {
        let s = format_timestamp(37623.0, "%Y");
        assert_eq!(s, "2003");
    }

    #[test]
    fn format_month() {
        let s = format_timestamp(37623.0, "%m");
        assert_eq!(s, "01");
    }

    #[test]
    fn format_day() {
        let s = format_timestamp(37623.0, "%d");
        assert_eq!(s, "02");
    }

    #[test]
    fn format_hour() {
        let s = format_timestamp(37623.5, "%H");
        assert_eq!(s, "12");
    }

    #[test]
    fn format_minute() {
        // 37623 + 30min/(24*60) = 37623 + 0.020833...
        let ts = 37623.0 + 30.0 / 1440.0;
        let s = format_timestamp(ts, "%M");
        assert_eq!(s, "30");
    }

    #[test]
    fn format_second() {
        // 37623 + 45sec/86400
        let ts = 37623.0 + 45.0 / 86400.0;
        let s = format_timestamp(ts, "%S");
        assert_eq!(s, "45");
    }

    #[test]
    fn format_percent_literal() {
        let s = format_timestamp(37623.0, "%%");
        assert_eq!(s, "%");
    }

    #[test]
    fn format_custom_dmy() {
        let s = format_timestamp(37623.0, "%d/%m/%Y");
        assert_eq!(s, "02/01/2003");
    }

    #[test]
    fn format_full_datetime() {
        let ts = 37623.5;
        let s = format_timestamp(ts, "%Y-%m-%d %H:%M:%S");
        assert_eq!(s, "2003-01-02 12:00:00");
    }

    #[test]
    fn day_one() {
        // Day 1 = 1899-12-31
        let (y, m, d, _, _, _) = timestamp_to_parts(1.0);
        assert_eq!((y, m, d), (1899, 12, 31));
    }

    #[test]
    fn day_two() {
        // Day 2 = 1900-01-01
        let (y, m, d, _, _, _) = timestamp_to_parts(2.0);
        assert_eq!((y, m, d), (1900, 1, 1));
    }

    #[test]
    fn nan_returns_epoch() {
        assert_eq!(timestamp_to_parts(f64::NAN), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn infinity_returns_epoch() {
        assert_eq!(timestamp_to_parts(f64::INFINITY), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn neg_infinity_returns_epoch() {
        assert_eq!(
            timestamp_to_parts(f64::NEG_INFINITY),
            (1899, 12, 30, 0, 0, 0)
        );
    }

    #[test]
    fn is_date_only_nan() {
        assert!(is_date_only(f64::NAN));
    }

    #[test]
    fn is_date_only_infinity() {
        assert!(is_date_only(f64::INFINITY));
    }

    #[test]
    fn format_unknown_specifier() {
        let s = format_timestamp(37623.0, "%Z");
        assert_eq!(s, "%Z");
    }

    #[test]
    fn format_trailing_percent() {
        let s = format_timestamp(37623.0, "end%");
        assert!(s.ends_with('%'));
    }

    #[test]
    fn format_no_specifiers() {
        let s = format_timestamp(37623.0, "plain text");
        assert_eq!(s, "plain text");
    }

    #[test]
    fn format_empty_string() {
        let s = format_timestamp(37623.0, "");
        assert_eq!(s, "");
    }
}
