use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::format::{ColumnType, PageType};
use crate::map;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single column definition parsed from a TDEF page.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub col_num: u16,
    pub var_col_num: u16,
    pub fixed_offset: u16,
    pub col_size: u16,
    pub flags: u8,
    pub is_fixed: bool,
    /// Scale for Numeric columns (number of decimal places).
    pub scale: u8,
    /// Precision for Numeric columns.
    pub precision: u8,
}

/// A parsed table definition.
#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,
    pub num_rows: u32,
    pub num_cols: u16,
    pub num_var_cols: u16,
    pub columns: Vec<ColumnDef>,
    pub data_pages: Vec<u32>,
}

// ---------------------------------------------------------------------------
// read_table_def
// ---------------------------------------------------------------------------

/// Read and parse a table definition (TDEF) from the database.
///
/// `name` is the table name (stored in the returned `TableDef`).
/// `tdef_page` is the first TDEF page number.
pub fn read_table_def(
    reader: &mut PageReader,
    name: &str,
    tdef_page: u32,
) -> Result<TableDef, FileError> {
    let is_jet3 = reader.header().version.is_jet3();

    // 3a. Build TDEF buffer (multi-page support)
    let tdef_buf = build_tdef_buffer(reader, tdef_page)?;

    let format = reader.format();

    // 3b. Header fields
    let num_rows = read_u32(&tdef_buf, format.tdef_row_count_pos)?;
    let num_var_cols = read_u16(&tdef_buf, format.tdef_var_col_count_pos)?;
    let num_cols = read_u16(&tdef_buf, format.tdef_column_count_pos)?;
    let _num_idxs = read_u32(&tdef_buf, format.tdef_index_count_pos)?;
    let num_real_idxs = read_u32(&tdef_buf, format.tdef_real_index_count_pos)?;

    // 3c. Data pages via owned-pages usage map
    let pg_row = read_u32(&tdef_buf, format.tdef_owned_pages_pos)?;
    let data_pages = if pg_row != 0 {
        let map_data = reader.read_pg_row(pg_row)?;
        map::collect_page_numbers(reader, &map_data)?
    } else {
        Vec::new()
    };

    // 3d. Column entries
    let col_entry_start =
        format.tdef_index_entries_pos + (num_real_idxs as usize) * format.tdef_index_entry_span;
    let col_entry_span = format.tdef_column_entry_span;

    let mut columns = Vec::with_capacity(num_cols as usize);
    for i in 0..num_cols as usize {
        let offset = col_entry_start + i * col_entry_span;
        let col = tdef_buf
            .get(offset..offset + col_entry_span)
            .ok_or(FileError::InvalidTableDef {
                reason: "column entry extends beyond TDEF buffer",
            })?;

        let col_type = ColumnType::try_from(col[0])?;

        // col_num: Jet3 = 1 byte, Jet4 = 2 bytes LE
        // var_col_num: always 2 bytes LE
        let (col_num, var_col_num) = if is_jet3 {
            (
                col[format.coldef_number_pos] as u16,
                u16::from_le_bytes([
                    col[format.coldef_var_col_index_pos],
                    col[format.coldef_var_col_index_pos + 1],
                ]),
            )
        } else {
            (
                u16::from_le_bytes([
                    col[format.coldef_number_pos],
                    col[format.coldef_number_pos + 1],
                ]),
                u16::from_le_bytes([
                    col[format.coldef_var_col_index_pos],
                    col[format.coldef_var_col_index_pos + 1],
                ]),
            )
        };

        let flags = col[format.coldef_flags_pos];
        let is_fixed = (flags & crate::format::column_flags::FIXED) != 0;

        // fixed_offset: always 2 bytes LE
        let fixed_offset = u16::from_le_bytes([
            col[format.coldef_fixed_data_pos],
            col[format.coldef_fixed_data_pos + 1],
        ]);

        let col_size = u16::from_le_bytes([
            col[format.coldef_length_pos],
            col[format.coldef_length_pos + 1],
        ]);

        let scale = col[format.coldef_scale_pos];
        let precision = col[format.coldef_precision_pos];

        columns.push(ColumnDef {
            name: String::new(), // filled in step 3e
            col_type,
            col_num,
            var_col_num,
            fixed_offset,
            col_size,
            flags,
            is_fixed,
            scale,
            precision,
        });
    }

    // 3e. Column names (immediately after column entries)
    // TDEF layout: real_index_entries → column_entries → column_names
    let mut name_offset = col_entry_start + (num_cols as usize) * col_entry_span;

    for col in &mut columns {
        if is_jet3 {
            // Jet3: [len: u8][Latin-1 bytes]
            if name_offset >= tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "column name length extends beyond TDEF buffer",
                });
            }
            let name_len = tdef_buf[name_offset] as usize;
            name_offset += 1;
            let name_end = name_offset + name_len;
            if name_end > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "column name extends beyond TDEF buffer",
                });
            }
            col.name = encoding::decode_latin1(&tdef_buf[name_offset..name_end]);
            name_offset = name_end;
        } else {
            // Jet4: [len: u16 LE][UTF-16LE bytes]
            if name_offset + 2 > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "column name length extends beyond TDEF buffer",
                });
            }
            let name_len = u16::from_le_bytes([
                tdef_buf[name_offset],
                tdef_buf[name_offset + 1],
            ]) as usize;
            name_offset += 2;
            let name_end = name_offset + name_len;
            if name_end > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "column name extends beyond TDEF buffer",
                });
            }
            col.name = encoding::decode_utf16le(&tdef_buf[name_offset..name_end])
                .map_err(|_| FileError::InvalidTableDef {
                    reason: "invalid UTF-16LE column name",
                })?;
            name_offset = name_end;
        }
    }

    // 3f. Sort by col_num
    columns.sort_by_key(|c| c.col_num);

    Ok(TableDef {
        name: name.to_string(),
        num_rows,
        num_cols,
        num_var_cols,
        columns,
        data_pages,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Build a contiguous TDEF buffer by following the next-page chain.
fn build_tdef_buffer(reader: &mut PageReader, tdef_page: u32) -> Result<Vec<u8>, FileError> {
    let first_page = reader.read_page_copy(tdef_page)?;

    // Validate page type
    if first_page.is_empty() || first_page[0] != PageType::TableDefinition as u8 {
        return Err(FileError::InvalidTableDef {
            reason: "first page is not a TableDefinition page",
        });
    }

    // Next-page pointer at offset 4 of the first page
    let mut next = u32::from_le_bytes([first_page[4], first_page[5], first_page[6], first_page[7]]);
    let mut buf = first_page;

    // Follow continuation pages (skip their 8-byte header)
    while next != 0 {
        let cont_page = reader.read_page_copy(next)?;
        if cont_page.len() > 8 {
            buf.extend_from_slice(&cont_page[8..]);
        }
        next = u32::from_le_bytes([cont_page[4], cont_page[5], cont_page[6], cont_page[7]]);
    }

    Ok(buf)
}

fn read_u16(buf: &[u8], pos: usize) -> Result<u16, FileError> {
    buf.get(pos..pos + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(FileError::InvalidTableDef {
            reason: "TDEF buffer too short for u16 read",
        })
}

fn read_u32(buf: &[u8], pos: usize) -> Result<u32, FileError> {
    buf.get(pos..pos + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(FileError::InvalidTableDef {
            reason: "TDEF buffer too short for u32 read",
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::CATALOG_PAGE;

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

    fn assert_msysobjects(tdef: &TableDef) {
        assert!(
            tdef.num_cols > 0,
            "MSysObjects should have at least one column"
        );

        let col_names: Vec<&str> = tdef.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            col_names.contains(&"Id"),
            "MSysObjects should have 'Id' column, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"Name"),
            "MSysObjects should have 'Name' column, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"Type"),
            "MSysObjects should have 'Type' column, found: {col_names:?}"
        );

        assert!(
            !tdef.data_pages.is_empty(),
            "MSysObjects should have at least one data page"
        );
    }

    #[test]
    fn jet3_msysobjects() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn jet4_msysobjects() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn ace12_msysobjects() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn ace14_msysobjects() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn columns_sorted_by_col_num() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        for w in tdef.columns.windows(2) {
            assert!(
                w[0].col_num <= w[1].col_num,
                "columns should be sorted by col_num"
            );
        }
    }

    #[test]
    fn invalid_page_type_error() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Page 1 is typically a data/bitmap page, not TDEF
        let result = read_table_def(&mut reader, "bad", 1);
        assert!(result.is_err());
    }
}
