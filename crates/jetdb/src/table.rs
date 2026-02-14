use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::format::{ColumnType, JetFormat, PageType, MAX_INDEX_COLUMNS};
use crate::map;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Sort order for an index column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexColumnOrder {
    Ascending,
    Descending,
}

/// A single column within an index definition.
#[derive(Debug, Clone)]
pub struct IndexColumn {
    /// Column number (corresponds to `ColumnDef.col_num`).
    pub col_num: u16,
    /// Sort order.
    pub order: IndexColumnOrder,
}

/// Foreign key reference information (for `index_type == 2`).
#[derive(Debug, Clone)]
pub struct ForeignKeyReference {
    /// FK index type (0x00 or 0x01).
    pub fk_index_type: u8,
    /// FK index number.
    pub fk_index_number: u32,
    /// FK table page number.
    pub fk_table_page: u32,
    /// Update action flag.
    pub update_action: u8,
    /// Delete action flag.
    pub delete_action: u8,
}

/// A single index definition parsed from a TDEF page.
#[derive(Debug, Clone)]
pub struct IndexDef {
    /// Index name.
    pub name: String,
    /// Logical index number.
    pub index_num: u16,
    /// Index type: 0x01 = normal/PK, 0x02 = FK reference.
    pub index_type: u8,
    /// Columns in this index (empty for FK type=2).
    pub columns: Vec<IndexColumn>,
    /// Index flags (UNIQUE, IGNORE_NULLS, REQUIRED).
    pub flags: u8,
    /// B-tree root page number (0 for FK type=2 indexes).
    pub first_data_page: u32,
    /// Foreign key info (only for type=2 indexes).
    pub foreign_key: Option<ForeignKeyReference>,
}

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
    pub indexes: Vec<IndexDef>,
    pub data_pages: Vec<u32>,
}

/// Return `true` if the column has the REPLICATION flag set.
pub fn is_replication_column(col: &ColumnDef) -> bool {
    (col.flags & crate::format::column_flags::REPLICATION) != 0
}

// ---------------------------------------------------------------------------
// Private types
// ---------------------------------------------------------------------------

/// Physical index entry: (columns, flags, first_data_page).
type PhysicalIndexEntry = (Vec<IndexColumn>, u8, u32);

/// Logical index entry parsed from TDEF section [6].
struct LogicalIndex {
    index_num: u16,
    index_col_entry: u32,
    fk_index_type: u8,
    fk_index_number: u32,
    fk_table_page: u32,
    update_action: u8,
    delete_action: u8,
    index_type: u8,
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
    let num_idxs = read_u32(&tdef_buf, format.tdef_index_count_pos)?;
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
    let mut columns = parse_column_entries(
        &tdef_buf,
        col_entry_start,
        format.tdef_column_entry_span,
        num_cols as usize,
        is_jet3,
        format,
    )?;

    // 3e. Column names
    let mut offset = col_entry_start + (num_cols as usize) * format.tdef_column_entry_span;
    let (col_names, new_offset) = read_names(&tdef_buf, offset, num_cols as usize, is_jet3)?;
    for (col, col_name) in columns.iter_mut().zip(col_names) {
        col.name = col_name;
    }
    offset = new_offset;

    // 3f. Index column definitions
    let (mut idx_col_defs, new_offset) =
        parse_index_column_defs(&tdef_buf, offset, num_real_idxs, format)?;
    offset = new_offset;

    // 3g. Logical index definitions
    let (logical_indexes, new_offset) =
        parse_logical_indexes(&tdef_buf, offset, num_idxs, format)?;
    offset = new_offset;

    // Adjust idx_col_defs length based on actual non-FK count in section [6].
    let non_fk_count = logical_indexes
        .iter()
        .filter(|li| li.index_type != crate::format::index_type::FOREIGN_KEY)
        .count();
    if non_fk_count != idx_col_defs.len() {
        idx_col_defs.truncate(non_fk_count);
    }

    // 3h. Index names
    let (idx_names, _) = read_names(&tdef_buf, offset, num_idxs as usize, is_jet3)?;

    // 3i. Build index defs
    let indexes = build_index_defs(&logical_indexes, &idx_col_defs, idx_names);

    // 3j. Sort columns by col_num
    columns.sort_by_key(|c| c.col_num);

    Ok(TableDef {
        name: name.to_string(),
        num_rows,
        num_cols,
        num_var_cols,
        columns,
        indexes,
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

/// Read a sequence of names from the TDEF buffer.
///
/// Jet3 uses `[len: u8][Latin-1 bytes]`, Jet4+ uses `[len: u16 LE][UTF-16LE bytes]`.
/// Returns the names and the offset after the last name.
fn read_names(
    buf: &[u8],
    mut offset: usize,
    count: usize,
    is_jet3: bool,
) -> Result<(Vec<String>, usize), FileError> {
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        if is_jet3 {
            if offset >= buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "name length extends beyond TDEF buffer",
                });
            }
            let name_len = buf[offset] as usize;
            offset += 1;
            let name_end = offset + name_len;
            if name_end > buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "name extends beyond TDEF buffer",
                });
            }
            names.push(encoding::decode_latin1(&buf[offset..name_end]));
            offset = name_end;
        } else {
            if offset + 2 > buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "name length extends beyond TDEF buffer",
                });
            }
            let name_len =
                u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            let name_end = offset + name_len;
            if name_end > buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "name extends beyond TDEF buffer",
                });
            }
            names.push(
                encoding::decode_utf16le(&buf[offset..name_end]).map_err(|_| {
                    FileError::InvalidTableDef {
                        reason: "invalid UTF-16LE name",
                    }
                })?,
            );
            offset = name_end;
        }
    }
    Ok((names, offset))
}

/// Parse column definition entries from the TDEF buffer.
fn parse_column_entries(
    buf: &[u8],
    start: usize,
    span: usize,
    count: usize,
    is_jet3: bool,
    format: &JetFormat,
) -> Result<Vec<ColumnDef>, FileError> {
    let mut columns = Vec::with_capacity(count);
    for i in 0..count {
        let offset = start + i * span;
        let col = buf
            .get(offset..offset + span)
            .ok_or(FileError::InvalidTableDef {
                reason: "column entry extends beyond TDEF buffer",
            })?;

        let col_type = ColumnType::try_from(col[0])?;

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
            name: String::new(), // filled by read_names
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
    Ok(columns)
}

/// Parse index column definitions from TDEF section [5].
fn parse_index_column_defs(
    buf: &[u8],
    mut offset: usize,
    count: u32,
    format: &JetFormat,
) -> Result<(Vec<PhysicalIndexEntry>, usize), FileError> {
    let mut idx_col_defs = Vec::with_capacity(count as usize);

    for _ in 0..count {
        if offset + format.idx_col_block_size > buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index column definition entry extends beyond TDEF buffer",
            });
        }

        offset += format.idx_col_skip_before;

        let mut idx_columns = Vec::new();
        for _ in 0..MAX_INDEX_COLUMNS {
            if offset + 3 > buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "index column slot extends beyond TDEF buffer",
                });
            }
            let col_id = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
            let order_flag = buf[offset + 2];
            offset += 3;

            if col_id != 0xFFFF {
                let order = if order_flag == 0x01 {
                    IndexColumnOrder::Ascending
                } else {
                    IndexColumnOrder::Descending
                };
                idx_columns.push(IndexColumn {
                    col_num: col_id,
                    order,
                });
            }
        }

        if offset + 8 > buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index usage map / first page extends beyond TDEF buffer",
            });
        }
        offset += 4;

        let first_pg = u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]);
        offset += 4;

        offset += format.idx_col_skip_before_flags;

        if offset >= buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index flags extends beyond TDEF buffer",
            });
        }
        let idx_flags = buf[offset];
        offset += 1;

        offset += format.idx_col_skip_after_flags;

        idx_col_defs.push((idx_columns, idx_flags, first_pg));
    }

    Ok((idx_col_defs, offset))
}

/// Parse logical index definitions from TDEF section [6].
fn parse_logical_indexes(
    buf: &[u8],
    mut offset: usize,
    count: u32,
    format: &JetFormat,
) -> Result<(Vec<LogicalIndex>, usize), FileError> {
    let mut logical_indexes = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let entry_start = offset;
        if entry_start + format.idx_info_block_size > buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "logical index entry extends beyond TDEF buffer",
            });
        }
        let entry = &buf[entry_start..entry_start + format.idx_info_block_size];

        let skip = format.idx_info_skip_before;

        let index_num = u16::from_le_bytes([entry[skip], entry[skip + 1]]);
        let index_col_entry = u32::from_le_bytes([
            entry[skip + 4],
            entry[skip + 5],
            entry[skip + 6],
            entry[skip + 7],
        ]);
        let fk_index_type = entry[skip + 8];
        let fk_index_number = u32::from_le_bytes([
            entry[skip + 9],
            entry[skip + 10],
            entry[skip + 11],
            entry[skip + 12],
        ]);
        let fk_table_page = u32::from_le_bytes([
            entry[skip + 13],
            entry[skip + 14],
            entry[skip + 15],
            entry[skip + 16],
        ]);
        let update_action = entry[skip + 17];
        let delete_action = entry[skip + 18];
        let index_type = entry[format.idx_info_type_offset];

        logical_indexes.push(LogicalIndex {
            index_num,
            index_col_entry,
            fk_index_type,
            fk_index_number,
            fk_table_page,
            update_action,
            delete_action,
            index_type,
        });

        offset += format.idx_info_block_size;
    }

    Ok((logical_indexes, offset))
}

/// Combine logical indexes, column definitions, and names into `IndexDef` entries.
fn build_index_defs(
    logical_indexes: &[LogicalIndex],
    idx_col_defs: &[PhysicalIndexEntry],
    idx_names: Vec<String>,
) -> Vec<IndexDef> {
    let mut indexes = Vec::with_capacity(logical_indexes.len());
    for (i, logical) in logical_indexes.iter().enumerate() {
        let name = idx_names.get(i).cloned().unwrap_or_default();

        if logical.index_type == crate::format::index_type::FOREIGN_KEY {
            indexes.push(IndexDef {
                name,
                index_num: logical.index_num,
                index_type: logical.index_type,
                columns: Vec::new(),
                flags: 0,
                first_data_page: 0,
                foreign_key: Some(ForeignKeyReference {
                    fk_index_type: logical.fk_index_type,
                    fk_index_number: logical.fk_index_number,
                    fk_table_page: logical.fk_table_page,
                    update_action: logical.update_action,
                    delete_action: logical.delete_action,
                }),
            });
        } else {
            let col_entry_idx = logical.index_col_entry as usize;
            let (cols, flags, first_pg) = if col_entry_idx < idx_col_defs.len() {
                idx_col_defs[col_entry_idx].clone()
            } else {
                eprintln!(
                    "warning: index '{}' references out-of-range column def entry {} (max {})",
                    name,
                    col_entry_idx,
                    idx_col_defs.len()
                );
                (Vec::new(), 0, 0)
            };
            indexes.push(IndexDef {
                name,
                index_num: logical.index_num,
                index_type: logical.index_type,
                columns: cols,
                flags,
                first_data_page: first_pg,
                foreign_key: None,
            });
        }
    }
    indexes
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
    use crate::format::{column_flags, CATALOG_PAGE};
    use crate::format::ColumnType;

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

    // -- Index tests ----------------------------------------------------------

    /// Helper: find a user table's TDEF page from the catalog.
    fn find_table_page(reader: &mut PageReader, table_name: &str) -> Option<u32> {
        let catalog = crate::catalog::read_catalog(reader).ok()?;
        catalog
            .iter()
            .find(|e| e.name == table_name)
            .map(|e| e.table_page)
    }

    fn assert_user_table_indexes(path: &std::path::Path, table_name: &str) -> TableDef {
        let mut reader = PageReader::open(path).unwrap();
        let page = find_table_page(&mut reader, table_name)
            .unwrap_or_else(|| panic!("table '{table_name}' not found in catalog"));
        read_table_def(&mut reader, table_name, page).unwrap()
    }

    #[test]
    fn jet4_index_count() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "Table1 should have at least one index"
        );
    }

    #[test]
    fn jet3_index_count() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "Jet3 Table1 should have at least one index"
        );
    }

    #[test]
    fn ace12_index_count() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "ACE12 Table1 should have at least one index"
        );
    }

    #[test]
    fn ace14_index_count() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "ACE14 Table1 should have at least one index"
        );
    }

    #[test]
    fn jet4_primary_key() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let pk = tdef
            .indexes
            .iter()
            .find(|idx| idx.name == "PrimaryKey")
            .expect("Table1 should have a PrimaryKey index");

        assert_ne!(
            pk.flags & crate::format::index_flags::UNIQUE,
            0,
            "PrimaryKey should have UNIQUE flag"
        );
        assert_ne!(
            pk.flags & crate::format::index_flags::REQUIRED,
            0,
            "PrimaryKey should have REQUIRED flag"
        );
        assert!(
            !pk.columns.is_empty(),
            "PrimaryKey should have at least one column"
        );
    }

    #[test]
    fn jet4_index_columns() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        for idx in &tdef.indexes {
            if idx.index_type != crate::format::index_type::FOREIGN_KEY {
                assert!(
                    !idx.columns.is_empty(),
                    "non-FK index '{}' should have columns",
                    idx.name
                );
                for col in &idx.columns {
                    assert!(
                        (col.col_num as usize) < tdef.columns.len() + 256,
                        "index column number should be reasonable"
                    );
                }
            }
        }
    }

    #[test]
    fn index_fk_type() {
        // indexTestV2003.mdb has FK (type=2) indexes
        let path = skip_if_missing!("V2003/indexTestV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let fk_indexes: Vec<&IndexDef> = tdef
            .indexes
            .iter()
            .filter(|idx| idx.index_type == crate::format::index_type::FOREIGN_KEY)
            .collect();

        assert!(
            !fk_indexes.is_empty(),
            "indexTest Table1 should have FK indexes"
        );

        for fk in &fk_indexes {
            assert!(
                fk.foreign_key.is_some(),
                "FK index '{}' should have foreign_key info",
                fk.name
            );
            assert!(
                fk.columns.is_empty(),
                "FK index '{}' should have no columns",
                fk.name
            );
        }
    }

    #[test]
    fn jet3_index_fk_type() {
        let path = skip_if_missing!("V1997/indexTestV1997.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let fk_indexes: Vec<&IndexDef> = tdef
            .indexes
            .iter()
            .filter(|idx| idx.index_type == crate::format::index_type::FOREIGN_KEY)
            .collect();

        assert!(
            !fk_indexes.is_empty(),
            "Jet3 indexTest Table1 should have FK indexes"
        );
        for fk in &fk_indexes {
            assert!(fk.foreign_key.is_some());
        }
    }

    // -- is_replication_column tests ----------------------------------------

    #[test]
    fn is_replication_true() {
        let col = ColumnDef {
            name: "s_GUID".to_string(),
            col_type: ColumnType::Guid,
            col_num: 1,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 16,
            flags: column_flags::REPLICATION | column_flags::NULLABLE,
            is_fixed: false,
            precision: 0,
            scale: 0,
        };
        assert!(is_replication_column(&col));
    }

    #[test]
    fn is_replication_false() {
        let col = ColumnDef {
            name: "ID".to_string(),
            col_type: ColumnType::Long,
            col_num: 1,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags: column_flags::FIXED,
            is_fixed: true,
            precision: 0,
            scale: 0,
        };
        assert!(!is_replication_column(&col));
    }

    #[test]
    fn index_names_are_nonempty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        for idx in &tdef.indexes {
            assert!(
                !idx.name.is_empty(),
                "index name should not be empty"
            );
        }
    }

    // -- read_names tests -----------------------------------------------------

    #[test]
    fn read_names_jet3_latin1() {
        // Jet3 format: [len: u8][Latin-1 bytes]
        let buf = [3, b'F', b'o', b'o', 3, b'B', b'a', b'r'];
        let (names, offset) = read_names(&buf, 0, 2, true).unwrap();
        assert_eq!(names, vec!["Foo", "Bar"]);
        assert_eq!(offset, 8);
    }

    #[test]
    fn read_names_jet4_utf16le() {
        // Jet4 format: [len: u16 LE][UTF-16LE bytes]
        // "Ab" = 4 bytes UTF-16LE
        let buf = [
            4, 0, // len=4
            b'A', 0, b'b', 0, // "Ab"
            2, 0, // len=2
            b'X', 0, // "X"
        ];
        let (names, offset) = read_names(&buf, 0, 2, false).unwrap();
        assert_eq!(names, vec!["Ab", "X"]);
        assert_eq!(offset, 10);
    }

    #[test]
    fn read_names_boundary_error() {
        // Buffer too short for the name length prefix
        let buf = [3, b'A', b'B'];
        let result = read_names(&buf, 0, 1, true);
        assert!(result.is_err());
    }

    #[test]
    fn read_names_empty_count() {
        let buf = [];
        let (names, offset) = read_names(&buf, 0, 0, true).unwrap();
        assert!(names.is_empty());
        assert_eq!(offset, 0);
    }

    // -- parse_column_entries tests -------------------------------------------

    #[test]
    fn parse_column_entries_jet3() {
        use crate::format::JET3;
        // Build a minimal Jet3 column entry (18 bytes)
        let mut entry = vec![0u8; JET3.tdef_column_entry_span];
        entry[0] = ColumnType::Long.to_byte(); // col_type
        entry[JET3.coldef_number_pos] = 5; // col_num (1 byte for Jet3)
        entry[JET3.coldef_flags_pos] = column_flags::FIXED;
        entry[JET3.coldef_length_pos] = 4;
        entry[JET3.coldef_length_pos + 1] = 0;

        let cols = parse_column_entries(&entry, 0, JET3.tdef_column_entry_span, 1, true, &JET3).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].col_type, ColumnType::Long);
        assert_eq!(cols[0].col_num, 5);
        assert!(cols[0].is_fixed);
        assert_eq!(cols[0].col_size, 4);
    }

    #[test]
    fn parse_column_entries_jet4() {
        use crate::format::JET4;
        // Build a minimal Jet4 column entry (25 bytes)
        let mut entry = vec![0u8; JET4.tdef_column_entry_span];
        entry[0] = ColumnType::Text.to_byte();
        // col_num: 2 bytes LE
        entry[JET4.coldef_number_pos] = 3;
        entry[JET4.coldef_number_pos + 1] = 0;
        entry[JET4.coldef_flags_pos] = column_flags::NULLABLE;
        entry[JET4.coldef_length_pos] = 0xFF;
        entry[JET4.coldef_length_pos + 1] = 0;

        let cols = parse_column_entries(&entry, 0, JET4.tdef_column_entry_span, 1, false, &JET4).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].col_type, ColumnType::Text);
        assert_eq!(cols[0].col_num, 3);
        assert!(!cols[0].is_fixed);
        assert_eq!(cols[0].col_size, 255);
    }
}
