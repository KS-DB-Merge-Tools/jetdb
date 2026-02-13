use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::format::{ColumnType, PageType, MAX_INDEX_COLUMNS};
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

    // 3f. Section [5]: Index column definitions (num_real_idxs entries)
    //     Each entry: Jet3=39B, Jet4=52B
    let mut idx_col_defs: Vec<(Vec<IndexColumn>, u8, u32)> =
        Vec::with_capacity(num_real_idxs as usize);

    for _ in 0..num_real_idxs {
        // Verify the entire entry fits within the buffer (M-3 overflow protection)
        if name_offset + format.idx_col_block_size > tdef_buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index column definition entry extends beyond TDEF buffer",
            });
        }

        // Skip type marker (Jet4 only)
        name_offset += format.idx_col_skip_before;

        // Read up to MAX_INDEX_COLUMNS column slots
        let mut idx_columns = Vec::new();
        for _ in 0..MAX_INDEX_COLUMNS {
            if name_offset + 3 > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "index column slot extends beyond TDEF buffer",
                });
            }
            let col_id = u16::from_le_bytes([
                tdef_buf[name_offset],
                tdef_buf[name_offset + 1],
            ]);
            let order_flag = tdef_buf[name_offset + 2];
            name_offset += 3;

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

        // Usage map reference (4 bytes)
        if name_offset + 8 > tdef_buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index usage map / first page extends beyond TDEF buffer",
            });
        }
        name_offset += 4;

        // First index page — B-tree root page number (4 bytes)
        let first_pg = u32::from_le_bytes([
            tdef_buf[name_offset],
            tdef_buf[name_offset + 1],
            tdef_buf[name_offset + 2],
            tdef_buf[name_offset + 3],
        ]);
        name_offset += 4;

        // Jet4 only: 4 bytes unknown
        name_offset += format.idx_col_skip_before_flags;

        // flags (1 byte)
        // The flags field is documented as 2 bytes in some references,
        // but only the low byte carries meaningful flag bits (UNIQUE, IGNORE_NULLS, REQUIRED).
        // The high byte is always 0x00 in practice.
        if name_offset >= tdef_buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "index flags extends beyond TDEF buffer",
            });
        }
        let idx_flags = tdef_buf[name_offset];
        name_offset += 1;

        // Jet4 only: 5 bytes unknown
        name_offset += format.idx_col_skip_after_flags;

        idx_col_defs.push((idx_columns, idx_flags, first_pg));
    }

    // 3g. Section [6]: Logical index definitions (num_idxs entries)
    //     Each entry: Jet3=20B, Jet4=28B
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

    let mut logical_indexes: Vec<LogicalIndex> = Vec::with_capacity(num_idxs as usize);

    for _ in 0..num_idxs {
        let entry_start = name_offset;
        if entry_start + format.idx_info_block_size > tdef_buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "logical index entry extends beyond TDEF buffer",
            });
        }
        let entry = &tdef_buf[entry_start..entry_start + format.idx_info_block_size];

        // Skip type marker (Jet4 only)
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

        name_offset += format.idx_info_block_size;
    }

    // Adjust idx_col_defs length based on actual non-FK count in section [6].
    // This adjustment is applied (num_real_idxs vs type!=2 count);
    // without it, corrupt databases may cause out-of-bounds lookups.
    let non_fk_count = logical_indexes
        .iter()
        .filter(|li| li.index_type != crate::format::index_type::FOREIGN_KEY)
        .count();
    if non_fk_count != idx_col_defs.len() {
        idx_col_defs.truncate(non_fk_count);
    }

    // 3h. Section [7]: Index names (num_idxs entries)
    let mut idx_names: Vec<String> = Vec::with_capacity(num_idxs as usize);
    for _ in 0..num_idxs {
        if is_jet3 {
            if name_offset >= tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "index name length extends beyond TDEF buffer",
                });
            }
            let name_len = tdef_buf[name_offset] as usize;
            name_offset += 1;
            let name_end = name_offset + name_len;
            if name_end > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "index name extends beyond TDEF buffer",
                });
            }
            idx_names.push(encoding::decode_latin1(&tdef_buf[name_offset..name_end]));
            name_offset = name_end;
        } else {
            if name_offset + 2 > tdef_buf.len() {
                return Err(FileError::InvalidTableDef {
                    reason: "index name length extends beyond TDEF buffer",
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
                    reason: "index name extends beyond TDEF buffer",
                });
            }
            idx_names.push(
                encoding::decode_utf16le(&tdef_buf[name_offset..name_end])
                    .map_err(|_| FileError::InvalidTableDef {
                        reason: "invalid UTF-16LE index name",
                    })?,
            );
            name_offset = name_end;
        }
    }

    // 3i. Combine sections [5][6][7] into IndexDef entries
    let mut indexes: Vec<IndexDef> = Vec::with_capacity(num_idxs as usize);
    for (i, logical) in logical_indexes.iter().enumerate() {
        let name = idx_names.get(i).cloned().unwrap_or_default();

        if logical.index_type == crate::format::index_type::FOREIGN_KEY {
            // FK reference — no physical index columns
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
            // Normal/PK — look up section [5] entry by index_col_entry
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
}
