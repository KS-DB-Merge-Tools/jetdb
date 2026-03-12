//! MSysAccessStorage table reading — shared infrastructure for VBA and form/report extraction.

use std::collections::HashSet;

use crate::catalog;
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::table;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single entry from the MSysAccessStorage table.
pub(crate) struct StorageEntry {
    pub id: i32,
    pub parent_id: i32,
    pub name: String,
    pub entry_type: i32,
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read all entries from the MSysAccessStorage system table.
pub(crate) fn read_storage_entries(
    reader: &mut PageReader,
) -> Result<Vec<StorageEntry>, FileError> {
    // Find MSysAccessStorage in the catalog
    let catalog = catalog::read_catalog(reader)?;
    let entry = match catalog.iter().find(|e| e.name == "MSysAccessStorage") {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let tdef = table::read_table_def(reader, &entry.name, entry.table_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysAccessStorage");

    // Locate column indices
    let (mut id_idx, mut parent_id_idx, mut name_idx, mut type_idx, mut lv_idx) =
        (None, None, None, None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Id" => id_idx = Some(i),
            "ParentId" => parent_id_idx = Some(i),
            "Name" => name_idx = Some(i),
            "Type" => type_idx = Some(i),
            "Lv" => lv_idx = Some(i),
            _ => {}
        }
    }

    let id_idx = id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Id column",
    })?;
    let parent_id_idx = parent_id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing ParentId column",
    })?;
    let name_idx = name_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Name column",
    })?;
    let type_idx = type_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Type column",
    })?;
    let lv_idx = lv_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Lv column",
    })?;

    let mut entries = Vec::new();
    for row in &result.rows {
        let id = match row.get(id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let parent_id = match row.get(parent_id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let name = match row.get(name_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let entry_type = match row.get(type_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let data = match row.get(lv_idx) {
            Some(Value::Binary(b)) => b.clone(),
            _ => Vec::new(),
        };

        entries.push(StorageEntry {
            id,
            parent_id,
            name,
            entry_type,
            data,
        });
    }

    Ok(entries)
}

/// Check if a storage entry is a storage (directory) vs stream (file).
///
/// Type values: 1 = storage, 2 = stream (observed in Access databases).
pub(crate) fn is_storage(entry: &StorageEntry) -> bool {
    entry.entry_type == 1
}

/// Recursively collect children of a given parent ID.
pub(crate) fn collect_children<'a>(
    entries: &'a [StorageEntry],
    parent_id: i32,
    result: &mut Vec<&'a StorageEntry>,
    visited: &mut HashSet<i32>,
) {
    for entry in entries {
        if entry.parent_id == parent_id && visited.insert(entry.id) {
            result.push(entry);
            collect_children(entries, entry.id, result, visited);
        }
    }
}

/// Build the CFB path for a storage entry relative to a given root.
///
/// Returns `None` if the parent chain is broken (circular reference or
/// missing parent), logging a warning so the caller can skip the entry.
pub(crate) fn build_entry_path(
    entry: &StorageEntry,
    root_id: i32,
    id_map: &std::collections::HashMap<i32, &StorageEntry>,
) -> Option<String> {
    let mut parts = vec![entry.name.clone()];
    let mut current_parent = entry.parent_id;
    let mut visited = HashSet::new();

    // Walk up the tree, stopping at the root (which is the CFB root)
    while current_parent != root_id {
        if !visited.insert(current_parent) {
            log::warn!(
                "skipping entry '{}': circular reference in parent chain",
                entry.name
            );
            return None;
        }
        match id_map.get(&current_parent) {
            Some(parent) => {
                parts.push(parent.name.clone());
                current_parent = parent.parent_id;
            }
            None => {
                log::warn!(
                    "skipping entry '{}': missing parent id {}",
                    entry.name,
                    current_parent
                );
                return None;
            }
        }
    }

    parts.reverse();
    Some(format!("/{}", parts.join("/")))
}
