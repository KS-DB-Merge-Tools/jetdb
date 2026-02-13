use crate::encoding;
use crate::file::{find_row, FileError, PageReader};
use crate::format::{row, ColumnType};
use crate::money;
use crate::table::{ColumnDef, TableDef};

// ---------------------------------------------------------------------------
// Value enum
// ---------------------------------------------------------------------------

/// A single column value read from a data row.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Byte(u8),
    Int(i16),
    Long(i32),
    BigInt(i64),
    Float(f32),
    Double(f64),
    Text(String),
    Binary(Vec<u8>),
    /// Money: fixed-point string with 4 decimal places (e.g. `"12345.6789"`).
    Money(String),
    /// Numeric: fixed-point string whose scale depends on the column definition.
    Numeric(String),
    /// Timestamp: f64 days since 1899-12-30.
    Timestamp(f64),
    /// GUID: `"{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}"` format.
    Guid(String),
}

// ---------------------------------------------------------------------------
// read_table_rows — public entry point
// ---------------------------------------------------------------------------

/// Result of reading data rows from a table.
pub struct ReadResult {
    /// Successfully parsed rows.
    pub rows: Vec<Vec<Value>>,
    /// Number of rows that were skipped due to parse errors.
    pub skipped_rows: usize,
}

/// Read all data rows from the table's data pages.
///
/// Returns a `ReadResult` containing the successfully parsed rows and a count
/// of rows that were skipped due to errors (e.g. corrupt row data).
pub fn read_table_rows(
    reader: &mut PageReader,
    table: &TableDef,
) -> Result<ReadResult, FileError> {
    let format = reader.format();
    let is_jet3 = reader.header().version.is_jet3();
    let mut rows = Vec::new();
    let mut skipped_rows = 0usize;

    for &page_num in &table.data_pages {
        let page_data = reader.read_page_copy(page_num)?;

        // Validate page type (Data = 1)
        if page_data.is_empty() || page_data[0] != 0x01 {
            continue;
        }

        let row_count_pos = format.data_row_count_pos;
        if page_data.len() < row_count_pos + 2 {
            continue;
        }
        let num_rows =
            u16::from_le_bytes([page_data[row_count_pos], page_data[row_count_pos + 1]]);

        for row_idx in 0..num_rows {
            // Read the raw row pointer to check flags before find_row
            let table_start = row_count_pos + 2;
            let entry_pos = table_start + (row_idx as usize) * 2;
            if entry_pos + 2 > page_data.len() {
                break;
            }
            let row_ptr =
                u16::from_le_bytes([page_data[entry_pos], page_data[entry_pos + 1]]);

            // Skip deleted rows
            if row_ptr & row::DELETE_FLAG != 0 {
                continue;
            }
            // Skip overflow/lookup rows (multi-page rows not yet supported)
            if row_ptr & row::LOOKUP_FLAG != 0 {
                continue;
            }

            let (start, size) = match find_row(format, &page_data, page_num, row_idx) {
                Ok(v) => v,
                Err(_) => {
                    skipped_rows += 1;
                    continue;
                }
            };

            let row_data = &page_data[start..start + size];
            let cracked = match crack_row(row_data, is_jet3) {
                Ok(c) => c,
                Err(_) => {
                    skipped_rows += 1;
                    continue;
                }
            };

            let mut values = Vec::with_capacity(table.columns.len());
            for col in &table.columns {
                let val = read_column_value(&cracked, col, is_jet3);
                values.push(val);
            }
            rows.push(values);
        }
    }

    Ok(ReadResult {
        rows,
        skipped_rows,
    })
}

// ---------------------------------------------------------------------------
// CrackedRow — parsed row structure
// ---------------------------------------------------------------------------

/// Parsed structure of a single data row.
#[allow(dead_code)]
struct CrackedRow<'a> {
    row_data: &'a [u8],
    col_count: u16,
    null_mask: &'a [u8],
    var_col_count: u16,
    /// Variable-column offset table, read backwards from the var_col_count
    /// position. In Jet4/ACE, variable data grows downward from the offset
    /// table, so lower-numbered variable columns have lower offsets.
    ///
    /// - `var_offsets[0]` = start offset of var col 0's data (the "EOD" marker)
    /// - `var_offsets[k]` = start of var col `k`'s data
    /// - `var_offsets[k+1]` = end of var col `k`'s data
    ///
    /// Data for variable column `k`: `row_data[var_offsets[k]..var_offsets[k+1]]`
    var_offsets: Vec<u16>,
}

// ---------------------------------------------------------------------------
// crack_row
// ---------------------------------------------------------------------------

/// Parse the internal structure of a data row.
fn crack_row<'a>(row_data: &'a [u8], is_jet3: bool) -> Result<CrackedRow<'a>, FileError> {
    if is_jet3 {
        crack_row_jet3(row_data)
    } else {
        crack_row_jet4(row_data)
    }
}

/// Jet4/ACE row layout (reading from the end):
/// ```text
/// [col_count: u16]           ← row start
/// [fixed data ...]
/// [variable data ...]
/// --- from end ---
/// [null_mask: ceil(col_count/8)]
/// [var_col_count: u16]
/// [eod: u16]                 ← end-of-data marker
/// [var_offset[N-1]: u16]
/// ...
/// [var_offset[0]: u16]
/// ```
///
/// The offset table is read **backwards** from `var_col_count` so that
/// `var_offsets[0] = EOD` and `var_offsets[k+1] = start of var col k`.
fn crack_row_jet4(row_data: &[u8]) -> Result<CrackedRow<'_>, FileError> {
    let len = row_data.len();
    if len < 2 {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for column count",
        });
    }

    let col_count = u16::from_le_bytes([row_data[0], row_data[1]]);
    let null_mask_len = (col_count as usize).div_ceil(8);

    // Read from end: null_mask, then var_col_count
    let tail_min = null_mask_len + 2; // null_mask + var_col_count
    if len < 2 + tail_min {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for null mask and var col count",
        });
    }

    let null_mask_start = len - null_mask_len;
    let null_mask = &row_data[null_mask_start..];

    let vcc_pos = null_mask_start - 2;
    let var_col_count = u16::from_le_bytes([row_data[vcc_pos], row_data[vcc_pos + 1]]);

    // Read offset table backwards from vcc_pos.
    // Entry count = var_col_count + 1 (includes EOD).
    // var_offsets[0] = EOD at (vcc_pos - 2)
    // var_offsets[k+1] = start offset of var col k at (vcc_pos - 2*(k+2))
    let offset_entries = var_col_count as usize + 1;
    let mut var_offsets = Vec::with_capacity(offset_entries);
    for i in 0..offset_entries {
        let pos = vcc_pos.wrapping_sub(2 + i * 2);
        if pos + 2 > len {
            break;
        }
        var_offsets.push(u16::from_le_bytes([row_data[pos], row_data[pos + 1]]));
    }

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count,
        var_offsets,
    })
}

/// Jet3 row layout (reading from the end):
/// ```text
/// [col_count: u8]            ← row start
/// [fixed data ...]
/// [variable data ...]
/// [offset_table ...]         ← 1 byte per entry, var_col_count+1 entries
/// --- from end ---
/// [null_mask: ceil(col_count/8)]
/// [var_col_count: u8]        ← null_mask の直前
/// [jump_table: num_jumps bytes]  ← var_col_count の直前
/// ```
///
/// Jump table entries contain **column numbers** (not page indices).
/// The dynamic `while` loop method is used to
/// resolve offsets that span 256-byte boundaries.
///
/// Same backward-read convention: `var_offsets[0] = EOD`, `var_offsets[k+1] = start of var col k`.
fn crack_row_jet3(row_data: &[u8]) -> Result<CrackedRow<'_>, FileError> {
    let len = row_data.len();
    if len < 1 {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for column count",
        });
    }

    let col_count = row_data[0] as u16;
    let null_mask_len = (col_count as usize).div_ceil(8);

    let null_mask_start = len - null_mask_len;
    if null_mask_start == 0 {
        return Ok(CrackedRow {
            row_data,
            col_count,
            null_mask: &row_data[null_mask_start..],
            var_col_count: 0,
            var_offsets: Vec::new(),
        });
    }
    let null_mask = &row_data[null_mask_start..];

    // var_col_count is at null_mask_start - 1
    let vcc_pos = null_mask_start - 1;
    if vcc_pos == 0 {
        return Ok(CrackedRow {
            row_data,
            col_count,
            null_mask,
            var_col_count: 0,
            var_offsets: Vec::new(),
        });
    }
    let var_col_count = row_data[vcc_pos] as u16;

    // Jump table is between var_col_count and the offset table.
    // num_jumps = (row_len - 1) / 256
    let num_jumps = if len > 1 { (len - 1) / 256 } else { 0 };

    // col_ptr = vcc_pos - num_jumps - 1 (start of offset table, reading backwards)
    let col_ptr = vcc_pos.saturating_sub(num_jumps + 1);

    // Offset entries: var_col_count + 1 (includes EOD), each 1 byte
    let offset_entries = var_col_count as usize + 1;

    // Dummy jump check:
    // If last jump is a dummy value, ignore it
    let mut actual_num_jumps = num_jumps;
    if actual_num_jumps > 0
        && col_ptr.saturating_sub(offset_entries) / 256 < actual_num_jumps
    {
        actual_num_jumps -= 1;
    }

    if col_ptr < offset_entries {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for variable offset table (Jet3)",
        });
    }

    // Read offsets using the dynamic while-loop method.
    // Jump table entries are at vcc_pos - 1 - k (for k = 0..actual_num_jumps-1)
    // and contain column numbers where jumps_used should increment.
    let mut var_offsets = Vec::with_capacity(offset_entries);
    let mut jumps_used = 0usize;
    for i in 0..offset_entries {
        while jumps_used < actual_num_jumps
            && i == row_data[vcc_pos - 1 - jumps_used] as usize
        {
            jumps_used += 1;
        }
        let raw_offset = row_data[col_ptr - i] as u16;
        var_offsets.push(raw_offset + (jumps_used as u16) * 256);
    }

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count,
        var_offsets,
    })
}

// ---------------------------------------------------------------------------
// Null mask
// ---------------------------------------------------------------------------

/// Check if a column is NULL based on the null bit mask.
///
/// Bit = 1 means NOT NULL; bit = 0 means NULL.
fn is_null(null_mask: &[u8], col_num: u16) -> bool {
    let byte_idx = col_num as usize / 8;
    let bit_idx = col_num as usize % 8;
    if byte_idx >= null_mask.len() {
        return true; // out of range → treat as null
    }
    (null_mask[byte_idx] & (1 << bit_idx)) == 0
}

// ---------------------------------------------------------------------------
// read_column_value
// ---------------------------------------------------------------------------

/// Read a single column value from a cracked row.
fn read_column_value(cracked: &CrackedRow<'_>, col: &ColumnDef, is_jet3: bool) -> Value {
    // Boolean is special: value comes from the null mask
    if col.col_type == ColumnType::Boolean {
        return Value::Bool(!is_null(cracked.null_mask, col.col_num));
    }

    // All other types: check null first
    if is_null(cracked.null_mask, col.col_num) {
        return Value::Null;
    }

    if col.is_fixed {
        read_fixed_value(cracked, col, is_jet3)
    } else {
        read_variable_value(cracked, col, is_jet3)
    }
}

/// Read a fixed-length column value.
fn read_fixed_value(cracked: &CrackedRow<'_>, col: &ColumnDef, is_jet3: bool) -> Value {
    let col_count_size = if is_jet3 { 1usize } else { 2usize };
    let offset = col_count_size + col.fixed_offset as usize;
    let data = cracked.row_data;

    match col.col_type {
        ColumnType::Boolean => unreachable!("handled above"),
        ColumnType::Byte => {
            if offset < data.len() {
                Value::Byte(data[offset])
            } else {
                Value::Null
            }
        }
        ColumnType::Int => {
            if offset + 2 <= data.len() {
                Value::Int(i16::from_le_bytes([data[offset], data[offset + 1]]))
            } else {
                Value::Null
            }
        }
        ColumnType::Long => {
            if offset + 4 <= data.len() {
                Value::Long(i32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]))
            } else {
                Value::Null
            }
        }
        ColumnType::BigInt => {
            if offset + 8 <= data.len() {
                Value::BigInt(i64::from_le_bytes(
                    data[offset..offset + 8].try_into().unwrap(),
                ))
            } else {
                Value::Null
            }
        }
        ColumnType::Float => {
            if offset + 4 <= data.len() {
                Value::Float(f32::from_le_bytes(
                    data[offset..offset + 4].try_into().unwrap(),
                ))
            } else {
                Value::Null
            }
        }
        ColumnType::Double => {
            if offset + 8 <= data.len() {
                Value::Double(f64::from_le_bytes(
                    data[offset..offset + 8].try_into().unwrap(),
                ))
            } else {
                Value::Null
            }
        }
        ColumnType::Money => {
            if offset + 8 <= data.len() {
                let bytes: [u8; 8] = data[offset..offset + 8].try_into().unwrap();
                Value::Money(money::money_to_string(&bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Numeric => {
            if offset + 17 <= data.len() {
                let bytes: [u8; 17] = data[offset..offset + 17].try_into().unwrap();
                Value::Numeric(money::numeric_to_string(&bytes, col.scale))
            } else {
                Value::Null
            }
        }
        ColumnType::Timestamp => {
            if offset + 8 <= data.len() {
                Value::Timestamp(f64::from_le_bytes(
                    data[offset..offset + 8].try_into().unwrap(),
                ))
            } else {
                Value::Null
            }
        }
        ColumnType::Guid => {
            if offset + 16 <= data.len() {
                Value::Guid(format_guid(&data[offset..offset + 16]))
            } else {
                Value::Null
            }
        }
        ColumnType::ComplexType => {
            if offset + 4 <= data.len() {
                Value::Long(i32::from_le_bytes(
                    data[offset..offset + 4].try_into().unwrap(),
                ))
            } else {
                Value::Null
            }
        }
        // Variable-length types should not reach here, but handle gracefully
        _ => Value::Null,
    }
}

/// Read a variable-length column value.
fn read_variable_value(cracked: &CrackedRow<'_>, col: &ColumnDef, is_jet3: bool) -> Value {
    // var_offsets is read backwards from vcc_pos:
    // Data for var col k: row_data[var_offsets[k]..var_offsets[k+1]]
    let var_idx = col.var_col_num as usize;

    // Need var_offsets[var_idx] (start) and var_offsets[var_idx+1] (end)
    if var_idx + 1 >= cracked.var_offsets.len() {
        return Value::Null;
    }

    let start = cracked.var_offsets[var_idx] as usize;
    let end = cracked.var_offsets[var_idx + 1] as usize;

    if start >= end || end > cracked.row_data.len() {
        return Value::Null;
    }

    let var_data = &cracked.row_data[start..end];

    match col.col_type {
        ColumnType::Text => {
            match encoding::decode_text(var_data, is_jet3) {
                Ok(s) => Value::Text(s),
                Err(_) => Value::Null,
            }
        }
        ColumnType::Binary => Value::Binary(var_data.to_vec()),
        ColumnType::Memo => read_memo_value(var_data, is_jet3),
        ColumnType::Ole => Value::Null, // not yet supported
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Memo / OLE long value types
// ---------------------------------------------------------------------------

/// Inline long value — data stored directly in the row.
const MEMO_INLINE: u32 = 0x80000000;
/// Single-page overflow — data stored on one other page.
const MEMO_SINGLE_PAGE: u32 = 0x40000000;
/// Mask for the type flag bits.
const MEMO_TYPE_MASK: u32 = 0xC0000000;
/// Byte offset where inline long value data begins (after length_with_flags + padding).
const MEMO_INLINE_HEADER: usize = 8;

/// Read a Memo field value.
///
/// Memo/OLE variable data starts with a 4-byte `length_with_flags` (u32 LE):
/// - bit 31 (0x80000000): LONG_VALUE_TYPE_THIS_PAGE — inline data
/// - bit 30 (0x40000000): LONG_VALUE_TYPE_OTHER_PAGE — single page reference
/// - both 0: LONG_VALUE_TYPE_OTHER_PAGES — multi-page chain
///
/// Inline layout: `[length_with_flags(4B)] [unknown/padding(4B)] [data...]`
fn read_memo_value(var_data: &[u8], is_jet3: bool) -> Value {
    if var_data.len() < 4 {
        return Value::Null;
    }
    let length_with_flags =
        u32::from_le_bytes(var_data[..4].try_into().unwrap());
    let memo_type = length_with_flags & MEMO_TYPE_MASK;
    let data_len = (length_with_flags & !MEMO_TYPE_MASK) as usize;

    if memo_type == MEMO_INLINE {
        // Inline: data starts at offset 8
        let data_start = MEMO_INLINE_HEADER.min(var_data.len());
        let data_end = (data_start + data_len).min(var_data.len());
        if data_start >= var_data.len() {
            return Value::Null;
        }
        match encoding::decode_text(&var_data[data_start..data_end], is_jet3) {
            Ok(s) => Value::Text(s),
            Err(_) => Value::Null,
        }
    } else if memo_type == MEMO_SINGLE_PAGE {
        // Single-page overflow — not yet supported
        Value::Null
    } else {
        // Multi-page overflow — not yet supported
        Value::Null
    }
}

// ---------------------------------------------------------------------------
// GUID formatting
// ---------------------------------------------------------------------------

/// Format 16 raw bytes as a GUID string.
///
/// The byte order follows the standard UUID mixed-endian layout:
/// `{AABBCCDD-EEFF-GGHH-IIJJ-KKLLMMNNOOPP}` where the first three groups
/// are byte-swapped.
fn format_guid(b: &[u8]) -> String {
    format!(
        "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        b[3], b[2], b[1], b[0],   // 4-byte swap
        b[5], b[4],               // 2-byte swap
        b[7], b[6],               // 2-byte swap
        b[8], b[9],               // as-is
        b[10], b[11], b[12], b[13], b[14], b[15], // as-is
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_null ---------------------------------------------------------------

    #[test]
    fn null_mask_bit_set_means_not_null() {
        // Byte 0 = 0b00000010 → col 1 is NOT NULL
        let mask = [0x02u8];
        assert!(!is_null(&mask, 1));
    }

    #[test]
    fn null_mask_bit_clear_means_null() {
        let mask = [0x02u8];
        assert!(is_null(&mask, 0)); // bit 0 = 0 → NULL
    }

    #[test]
    fn null_mask_out_of_range() {
        let mask = [0xFFu8];
        assert!(is_null(&mask, 8)); // byte_idx=1, beyond mask → null
    }

    // -- format_guid -----------------------------------------------------------

    #[test]
    fn guid_formatting() {
        let bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, // group 1
            0x05, 0x06, // group 2
            0x07, 0x08, // group 3
            0x09, 0x0A, // group 4
            0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, // group 5
        ];
        assert_eq!(
            format_guid(&bytes),
            "{04030201-0605-0807-090A-0B0C0D0E0F10}"
        );
    }

    #[test]
    fn guid_zero() {
        let bytes = [0u8; 16];
        assert_eq!(
            format_guid(&bytes),
            "{00000000-0000-0000-0000-000000000000}"
        );
    }

    // -- crack_row_jet4 --------------------------------------------------------

    #[test]
    fn crack_row_jet4_basic() {
        // Build a minimal Jet4 row with:
        //   col_count = 3, 1 fixed col (4 bytes), 1 var col
        //
        // Layout (forward):
        //   [0x03, 0x00]              ← col_count = 3
        //   [0xAA, 0xBB, 0xCC, 0xDD] ← fixed data (4 bytes)
        //   [0x48, 0x00, 0x69, 0x00]  ← var data "Hi" in UTF-16LE (offset 6..10)
        //   --- offset table (forward = descending order in Jet4) ---
        //   [end of var col 0 = 10]   ← furthest from vcc (highest offset)
        //   [start/EOD = 6]           ← closest to vcc (lowest offset)
        //   [var_col_count = 1]
        //   [null_mask = 0xFF]

        let mut row = Vec::new();
        // col_count
        row.extend_from_slice(&[0x03, 0x00]);
        // fixed data
        row.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        // var data: "Hi" in UTF-16LE
        row.extend_from_slice(&[0x48, 0x00, 0x69, 0x00]);
        // Offset table (forward/descending): end=10, start=6
        row.extend_from_slice(&10u16.to_le_bytes());
        row.extend_from_slice(&6u16.to_le_bytes());
        // var_col_count = 1
        row.extend_from_slice(&1u16.to_le_bytes());
        // null_mask: 1 byte, all bits set (not null)
        row.push(0xFF);

        let cracked = crack_row_jet4(&row).unwrap();
        assert_eq!(cracked.col_count, 3);
        assert_eq!(cracked.var_col_count, 1);
        // Backward read: var_offsets[0]=6 (start), var_offsets[1]=10 (end)
        assert_eq!(cracked.var_offsets, vec![6, 10]);
        assert_eq!(cracked.null_mask, &[0xFF]);
    }

    #[test]
    fn crack_row_jet4_no_var_cols() {
        // col_count = 2, no variable columns
        // fixed data: 2 bytes
        let mut row = Vec::new();
        row.extend_from_slice(&[0x02, 0x00]); // col_count
        row.extend_from_slice(&[0x42, 0x43]); // fixed data
        // EOD offset (points to end of fixed data = 4)
        row.extend_from_slice(&4u16.to_le_bytes());
        // var_col_count = 0
        row.extend_from_slice(&0u16.to_le_bytes());
        // null_mask: 1 byte
        row.push(0xFF);

        let cracked = crack_row_jet4(&row).unwrap();
        assert_eq!(cracked.col_count, 2);
        assert_eq!(cracked.var_col_count, 0);
        assert_eq!(cracked.var_offsets.len(), 1); // just EOD
    }

    // -- crack_row_jet3 --------------------------------------------------------

    #[test]
    fn crack_row_jet3_basic() {
        // Build a minimal Jet3 row with:
        //   col_count = 2, 1 fixed (2 bytes), 1 var col
        //
        // Jet3 end-of-row layout (from end):
        //   [null_mask]         ← row end
        //   [var_col_count]     ← null_mask の直前
        //   (no jump_table, row < 256 bytes)
        //   [offset_table]      ← var_col_count の直前
        //
        // Full layout:
        //   [0x02]              ← col_count = 2
        //   [0xAA, 0xBB]       ← fixed data
        //   [0x48, 0x69]       ← var data "Hi" in Latin-1 (offset 3..5)
        //   [5, 3]             ← offset table (end=5, EOD=3)
        //   [1]                ← var_col_count = 1
        //   [0xFF]             ← null_mask

        let mut row = Vec::new();
        row.push(0x02); // col_count
        row.extend_from_slice(&[0xAA, 0xBB]); // fixed data
        row.extend_from_slice(&[0x48, 0x69]); // var data
        // offset table: end=5, EOD=3
        row.push(5);
        row.push(3);
        // var_col_count = 1
        row.push(1);
        // null_mask = 1 byte
        row.push(0xFF);

        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 2);
        assert_eq!(cracked.var_col_count, 1);
        assert_eq!(cracked.var_offsets, vec![3, 5]);
    }

    #[test]
    fn crack_row_jet3_jump_table() {
        // Build a Jet3 row > 256 bytes to exercise the jump table logic.
        //
        // We simulate 2 variable columns whose data spans the 256-byte boundary.
        // row_len will be ~300 bytes, so num_jumps = (300-1)/256 = 1.
        //
        // col_count = 3, var_col_count = 2
        // var col 0 data: offsets 1..200   (within first 256 bytes)
        // var col 1 data: offsets 200..280 (crosses 256-byte boundary)
        //
        // Layout (from end):
        //   [null_mask: 1 byte]
        //   [var_col_count: 1 byte = 2]
        //   [jump_table: 1 byte]    ← column number where 256-boundary is crossed
        //   [offset_table: 3 bytes] ← 3 entries (var_col_count + 1)

        let col_count: u8 = 3;
        let var_col_count: u8 = 2;
        let null_mask_len = 1usize; // ceil(3/8) = 1

        // Target: var col 0 at [1..200], var col 1 at [200..280]
        // offset_table entries (read by index i):
        //   i=0: EOD = 1  (raw byte: 1)
        //   i=1: start of var col 0 end / var col 1 start = 200 (raw: 200)
        //   i=2: end of var col 1 = 280 (raw: 280 - 256 = 24, with jump correction)
        //
        // Jump table entry: column index where jumps_used increments.
        // jump entry contains the column number.
        // For i=2 (the 3rd entry), we need jumps_used=1,
        // so jump_table[0] = 2 (the column number that triggers the jump).

        // We'll construct the row as a fixed-size buffer.
        // Total row structure:
        //   [col_count(1)] [payload...] [offset_table(3)] [jump_table(1)] [vcc(1)] [null_mask(1)]
        // We need total length ~ 300. Let's target exactly 300.
        // Tail overhead = 3 + 1 + 1 + 1 = 6 bytes
        // Payload = 300 - 1 - 6 = 293 bytes (col_count + payload + tail = 300)

        let target_len = 300usize;
        let tail_size = (var_col_count as usize + 1) + 1 + 1 + null_mask_len; // offset_table + jump + vcc + null
        let payload_size = target_len - 1 - tail_size; // minus col_count byte

        let mut row = Vec::with_capacity(target_len);
        row.push(col_count);
        // Fill payload (fixed + variable data regions)
        row.extend(std::iter::repeat(0xAA).take(payload_size));

        // offset_table: 3 entries read via col_ptr - i.
        // col_ptr points to the last pushed byte (highest position).
        // Push in reverse order: entry[2] first, entry[0] last.
        row.push(24);  // col_ptr-2 → entry[2]: var col 1 end = 280 - 256 = 24
        row.push(200); // col_ptr-1 → entry[1]: var col 0 end / var col 1 start = 200
        row.push(1);   // col_ptr-0 → entry[0]: EOD = 1

        // jump_table: 1 entry — column number 2 triggers the jump
        row.push(2);   // jump_table[0] = 2

        // var_col_count
        row.push(var_col_count);

        // null_mask
        row.push(0xFF);

        assert_eq!(row.len(), target_len);

        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 3);
        assert_eq!(cracked.var_col_count, 2);

        // Expected offsets:
        // i=0: raw=1,   jumps_used=0 → 1 + 0*256 = 1
        // i=1: raw=200, jumps_used=0 → 200 + 0*256 = 200
        //   (jump entry is 2, i=1 ≠ 2 so no jump increment)
        // i=2: raw=24,  but first check jump: jump_table[0]=2, i=2 matches → jumps_used=1
        //   → 24 + 1*256 = 280
        assert_eq!(cracked.var_offsets, vec![1, 200, 280]);
    }

    // -- read_memo_value -------------------------------------------------------

    #[test]
    fn memo_inline_utf16le() {
        // Inline memo: length_with_flags has bit 31 set.
        // Text "Hi" in UTF-16LE = [0x48, 0x00, 0x69, 0x00] — 4 bytes.
        let data_len: u32 = 4;
        let flags: u32 = MEMO_INLINE | data_len;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes()); // length_with_flags
        var_data.extend_from_slice(&[0u8; 4]); // padding
        var_data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00]); // "Hi" UTF-16LE

        let val = read_memo_value(&var_data, false);
        assert_eq!(val, Value::Text("Hi".to_string()));
    }

    #[test]
    fn memo_inline_jet3_latin1() {
        // Jet3 inline memo: "Hi" in Latin-1 = [0x48, 0x69] — 2 bytes.
        let data_len: u32 = 2;
        let flags: u32 = MEMO_INLINE | data_len;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 4]); // padding
        var_data.extend_from_slice(&[0x48, 0x69]); // "Hi" Latin-1

        let val = read_memo_value(&var_data, true);
        assert_eq!(val, Value::Text("Hi".to_string()));
    }

    #[test]
    fn memo_overflow_returns_null() {
        // Single-page overflow (bit 30 set)
        let flags: u32 = MEMO_SINGLE_PAGE | 100;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]); // page ref + padding

        let val = read_memo_value(&var_data, false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_multi_page_returns_null() {
        // Multi-page overflow (no type bits set)
        let flags: u32 = 500; // no high bits
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]);

        let val = read_memo_value(&var_data, false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_too_short_returns_null() {
        // Less than 4 bytes
        let val = read_memo_value(&[0x01, 0x02], false);
        assert_eq!(val, Value::Null);
    }

    // -- read_column_value (Boolean) -------------------------------------------

    #[test]
    fn boolean_from_null_mask() {
        // null_mask bit 0 = 1 → Bool(true), bit 1 = 0 → Bool(false)
        let row_data = [0x02, 0x00]; // col_count = 2
        let cracked = CrackedRow {
            row_data: &row_data,
            col_count: 2,
            null_mask: &[0x01], // bit 0 set, bit 1 clear
            var_col_count: 0,
            var_offsets: Vec::new(),
        };

        let col0 = ColumnDef {
            name: "b0".into(),
            col_type: ColumnType::Boolean,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 1,
            flags: 0,
            is_fixed: true,
            scale: 0,
            precision: 0,
        };
        let col1 = ColumnDef {
            name: "b1".into(),
            col_type: ColumnType::Boolean,
            col_num: 1,
            ..col0.clone()
        };

        assert_eq!(read_column_value(&cracked, &col0, false), Value::Bool(true));
        assert_eq!(
            read_column_value(&cracked, &col1, false),
            Value::Bool(false)
        );
    }

    // -- read_column_value (fixed types) ---------------------------------------

    #[test]
    fn read_fixed_int_value() {
        // col_count=1 (2 bytes) + fixed data at offset 0
        let mut row_data = vec![0x01, 0x00]; // col_count
        row_data.extend_from_slice(&(-42i16).to_le_bytes());
        // tail: EOD offset + var_col_count + null_mask
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD (single entry)
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count=0
        row_data.push(0xFF); // null_mask

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "x".into(),
            col_type: ColumnType::Int,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 2,
            flags: 0x01, // FIXED
            is_fixed: true,
            scale: 0,
            precision: 0,
        };
        assert_eq!(read_column_value(&cracked, &col, false), Value::Int(-42));
    }

    #[test]
    fn read_fixed_long_value() {
        let mut row_data = vec![0x01, 0x00];
        row_data.extend_from_slice(&123456i32.to_le_bytes());
        row_data.extend_from_slice(&6u16.to_le_bytes());
        row_data.extend_from_slice(&0u16.to_le_bytes());
        row_data.push(0xFF);

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "id".into(),
            col_type: ColumnType::Long,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
        };
        assert_eq!(
            read_column_value(&cracked, &col, false),
            Value::Long(123456)
        );
    }

    #[test]
    fn read_guid_value() {
        let mut row_data = vec![0x01, 0x00]; // col_count
        // GUID bytes
        let guid_bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        row_data.extend_from_slice(&guid_bytes);
        row_data.extend_from_slice(&18u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes());
        row_data.push(0xFF);

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "g".into(),
            col_type: ColumnType::Guid,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 16,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
        };
        assert_eq!(
            read_column_value(&cracked, &col, false),
            Value::Guid("{04030201-0605-0807-090A-0B0C0D0E0F10}".to_string())
        );
    }

    // -- Integration tests with real files ------------------------------------

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data_path($path) {
                Some(p) => p,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    fn assert_msysobjects_rows(rows: &[Vec<Value>], table: &TableDef) {
        assert!(
            !rows.is_empty(),
            "MSysObjects should have at least one row"
        );

        // Find column indices
        let id_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Id")
            .expect("Id column");
        let name_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Name")
            .expect("Name column");
        let type_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Type")
            .expect("Type column");

        for row in rows {
            assert_eq!(row.len(), table.columns.len());

            // Id should be a non-null Long
            match &row[id_idx] {
                Value::Long(_) => {}
                other => panic!("Expected Long for Id, got: {other:?}"),
            }

            // Name should be a non-null non-empty Text
            match &row[name_idx] {
                Value::Text(s) => assert!(!s.is_empty(), "Name should not be empty"),
                other => panic!("Expected Text for Name, got: {other:?}"),
            }

            // Type should be a non-null Int
            match &row[type_idx] {
                Value::Int(_) => {}
                other => panic!("Expected Int for Type, got: {other:?}"),
            }
        }
    }

    #[test]
    fn jet3_msysobjects_rows() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn jet4_msysobjects_rows() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace12_msysobjects_rows() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace14_msysobjects_rows() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }
}
