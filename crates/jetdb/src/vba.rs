//! VBA project and module source code extraction.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read as _, Write};

use crate::catalog;
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::table;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// VBA module type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbaModuleType {
    Standard,
    ClassOrDocument,
}

/// A single VBA module with its source code.
#[derive(Debug, Clone)]
pub struct VbaModule {
    pub name: String,
    pub module_type: VbaModuleType,
    pub source: String,
}

/// A VBA project extracted from a database.
#[derive(Debug, Clone)]
pub struct VbaProject {
    pub modules: Vec<VbaModule>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read a VBA project from a Jet/ACE database.
///
/// Tries `MSysAccessStorage` (Jet4/ACE) first, then falls back to
/// `MSysAccessObjects` (Jet3/Access 97) for older databases.
pub fn read_vba_project(reader: &mut PageReader) -> Result<VbaProject, FileError> {
    // Try MSysAccessStorage first (Jet4/ACE format)
    let entries = read_storage_entries(reader)?;
    if !entries.is_empty() {
        let project = build_cfb_and_extract(&entries)?;
        if !project.modules.is_empty() {
            return Ok(project);
        }
    }

    // Fall back to MSysAccessObjects (Jet3/Access 97 format)
    let raw_cfb = read_access_objects_cfb(reader)?;
    if raw_cfb.is_empty() {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    }
    let cfb_bytes = extract_vba_project_cfb(raw_cfb)?;
    if cfb_bytes.is_empty() {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    }
    extract_modules_from_cfb(cfb_bytes)
}

/// Extract VBA modules from CFB bytes using the ovba crate.
fn extract_modules_from_cfb(cfb_bytes: Vec<u8>) -> Result<VbaProject, FileError> {
    let project = ovba::open_project(cfb_bytes).map_err(|e| FileError::InvalidVbaProject {
        reason: e.to_string(),
    })?;

    let mut modules = Vec::new();
    for module in &project.modules {
        let source =
            project
                .module_source(&module.name)
                .map_err(|e| FileError::InvalidVbaProject {
                    reason: e.to_string(),
                })?;

        let module_type = match module.module_type {
            ovba::ModuleType::Procedural => VbaModuleType::Standard,
            ovba::ModuleType::DocClsDesigner => VbaModuleType::ClassOrDocument,
        };

        modules.push(VbaModule {
            name: module.name.clone(),
            module_type,
            source,
        });
    }

    Ok(VbaProject { modules })
}

// ---------------------------------------------------------------------------
// Internal: MSysAccessStorage reading
// ---------------------------------------------------------------------------

/// A single entry from the MSysAccessStorage table.
struct StorageEntry {
    id: i32,
    parent_id: i32,
    name: String,
    entry_type: i32,
    data: Vec<u8>,
}

/// Read all entries from the MSysAccessStorage system table.
fn read_storage_entries(reader: &mut PageReader) -> Result<Vec<StorageEntry>, FileError> {
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

// ---------------------------------------------------------------------------
// Internal: MSysAccessObjects reading (Jet3 / Access 97)
// ---------------------------------------------------------------------------

/// Read the MSysAccessObjects table and reconstruct the full CFB.
///
/// In Access 97 format, all database objects (forms, reports, VBA, etc.) are
/// stored in a single large OLE2/CFB file split across multiple rows in
/// MSysAccessObjects. Row 0 is a metadata/directory entry; rows 1+ contain
/// the CFB data that should be concatenated in ID order.
fn read_access_objects_cfb(reader: &mut PageReader) -> Result<Vec<u8>, FileError> {
    let catalog = catalog::read_catalog(reader)?;
    let entry = match catalog.iter().find(|e| e.name == "MSysAccessObjects") {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let tdef = table::read_table_def(reader, &entry.name, entry.table_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysAccessObjects");

    // Locate column indices
    let (mut data_idx, mut id_idx) = (None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Data" => data_idx = Some(i),
            "ID" => id_idx = Some(i),
            _ => {}
        }
    }

    let data_idx = data_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessObjects missing Data column",
    })?;
    let id_idx = id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessObjects missing ID column",
    })?;

    // Collect rows with their IDs
    let mut rows: Vec<(i32, Vec<u8>)> = Vec::new();
    for row in &result.rows {
        let id = match row.get(id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let data = match row.get(data_idx) {
            Some(Value::Binary(b)) => b.clone(),
            _ => continue,
        };
        rows.push((id, data));
    }

    // Sort by ID, skip row 0 (metadata), concatenate
    rows.sort_by_key(|(id, _)| *id);

    let mut cfb_bytes = Vec::new();
    for (id, data) in &rows {
        if *id == 0 {
            continue;
        }
        cfb_bytes.extend_from_slice(data);
    }

    if cfb_bytes.len() < 4 || cfb_bytes[..4] != [0xD0, 0xCF, 0x11, 0xE0] {
        return Ok(Vec::new());
    }

    Ok(cfb_bytes)
}

/// Extract the VBAProject subtree from a CFB and rebuild it as a root-level CFB.
///
/// In MSysAccessObjects format, the VBA project lives at `/VBA/VBAProject/`
/// within the large CFB. ovba expects VBA entries at the root level
/// (e.g., `/VBA/dir`, `/PROJECT`), so we strip the `/VBA/VBAProject` prefix.
fn extract_vba_project_cfb(cfb_bytes: Vec<u8>) -> Result<Vec<u8>, FileError> {
    let cursor = Cursor::new(cfb_bytes);
    let mut source = cfb::CompoundFile::open(cursor).map_err(|e| FileError::InvalidVbaProject {
        reason: format!("failed to open CFB: {e}"),
    })?;

    const PREFIX: &str = "/VBA/VBAProject";

    // Collect entries under /VBA/VBAProject/
    let entries: Vec<(String, bool)> = source
        .walk()
        .filter_map(|e| {
            // Use '/' separators regardless of OS (CFB paths are internal, not filesystem)
            let path = e.path().to_string_lossy().replace('\\', "/");
            if path.starts_with(PREFIX) && path.len() > PREFIX.len() {
                let relative = &path[PREFIX.len()..];
                Some((relative.to_string(), e.is_storage()))
            } else {
                None
            }
        })
        .collect();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    // Build new CFB with entries at root level
    let out_cursor = Cursor::new(Vec::new());
    let mut dest =
        cfb::CompoundFile::create(out_cursor).map_err(|e| FileError::InvalidVbaProject {
            reason: format!("failed to create CFB: {e}"),
        })?;

    // Create storages first (ensures parent dirs exist)
    for (path, is_storage) in &entries {
        if *is_storage {
            dest.create_storage_all(path)
                .map_err(|e| FileError::InvalidVbaProject {
                    reason: format!("failed to create storage '{path}': {e}"),
                })?;
        }
    }

    // Then create streams with their data
    for (path, is_storage) in &entries {
        if !*is_storage {
            let source_path = format!("{PREFIX}{path}");

            let mut data = Vec::new();
            source
                .open_stream(&source_path)
                .map_err(|e| FileError::InvalidVbaProject {
                    reason: format!("failed to open stream '{source_path}': {e}"),
                })?
                .read_to_end(&mut data)
                .map_err(|e| FileError::InvalidVbaProject {
                    reason: format!("failed to read stream '{source_path}': {e}"),
                })?;

            let mut stream =
                dest.create_stream(path)
                    .map_err(|e| FileError::InvalidVbaProject {
                        reason: format!("failed to create stream '{path}': {e}"),
                    })?;
            stream
                .write_all(&data)
                .map_err(|e| FileError::InvalidVbaProject {
                    reason: format!("failed to write stream '{path}': {e}"),
                })?;
        }
    }

    dest.flush().map_err(|e| FileError::InvalidVbaProject {
        reason: format!("failed to flush CFB: {e}"),
    })?;

    Ok(dest.into_inner().into_inner())
}

// ---------------------------------------------------------------------------
// Internal: VBA subtree filtering
// ---------------------------------------------------------------------------

/// Find the VBAProject root entry and collect all its children.
///
/// The MSysAccessStorage tree for VBA typically looks like:
/// ```text
/// MSysAccessStorage_ROOT (id=1)
///   VBA (id=8)                    ← outer VBA storage
///     VBAProject (id=17)          ← CFB root equivalent
///       VBA (id=18)               ← inner VBA storage
///         dir, _VBA_PROJECT, module streams...
///       PROJECT, PROJECTwm
///     AcessVBAData                ← not needed
/// ```
///
/// Returns the VBAProject entry ID and all its descendant entries.
fn find_vba_project_entries(entries: &[StorageEntry]) -> Option<(i32, Vec<&StorageEntry>)> {
    // Find the "VBAProject" storage entry
    let vba_project = entries
        .iter()
        .find(|e| e.name == "VBAProject" && is_storage(e))?;

    let mut children = Vec::new();
    let mut visited = HashSet::new();
    collect_children(entries, vba_project.id, &mut children, &mut visited);
    Some((vba_project.id, children))
}

/// Recursively collect children of a given parent ID.
fn collect_children<'a>(
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

// ---------------------------------------------------------------------------
// Internal: CFB reconstruction
// ---------------------------------------------------------------------------

/// Build an in-memory OLE2/CFB from MSysAccessStorage entries and extract modules.
fn build_cfb_and_extract(entries: &[StorageEntry]) -> Result<VbaProject, FileError> {
    let (vba_project_id, vba_entries) = match find_vba_project_entries(entries) {
        Some(v) => v,
        None => {
            return Ok(VbaProject {
                modules: Vec::new(),
            })
        }
    };
    if vba_entries.is_empty() {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    }

    // Build an ID-to-entry map for path construction
    let id_map: HashMap<i32, &StorageEntry> = entries.iter().map(|e| (e.id, e)).collect();

    let cursor = Cursor::new(Vec::new());
    let mut cf = cfb::CompoundFile::create(cursor).map_err(|e| FileError::InvalidVbaProject {
        reason: format!("failed to create CFB: {e}"),
    })?;

    // Create storages first, then streams (ensures parent dirs exist)
    let storages: Vec<_> = vba_entries.iter().filter(|e| is_storage(e)).collect();
    let streams: Vec<_> = vba_entries.iter().filter(|e| !is_storage(e)).collect();

    for entry in &storages {
        let path = match build_entry_path(entry, vba_project_id, &id_map) {
            Some(p) => p,
            None => continue,
        };
        cf.create_storage_all(&path)
            .map_err(|e| FileError::InvalidVbaProject {
                reason: format!("failed to create storage '{path}': {e}"),
            })?;
    }

    for entry in &streams {
        let path = match build_entry_path(entry, vba_project_id, &id_map) {
            Some(p) => p,
            None => continue,
        };
        let mut stream = cf
            .create_stream(&path)
            .map_err(|e| FileError::InvalidVbaProject {
                reason: format!("failed to create stream '{path}': {e}"),
            })?;
        stream
            .write_all(&entry.data)
            .map_err(|e| FileError::InvalidVbaProject {
                reason: format!("failed to write stream '{path}': {e}"),
            })?;
    }

    cf.flush().map_err(|e| FileError::InvalidVbaProject {
        reason: format!("failed to flush CFB: {e}"),
    })?;

    let bytes = cf.into_inner().into_inner();
    extract_modules_from_cfb(bytes)
}

/// Check if a storage entry is a storage (directory) vs stream (file).
///
/// Type values: 1 = storage, 2 = stream (observed in Access databases).
fn is_storage(entry: &StorageEntry) -> bool {
    entry.entry_type == 1
}

/// Build the CFB path for a storage entry relative to the VBAProject root.
///
/// `vba_project_id` is the ID of the VBAProject storage entry, which
/// corresponds to the root of the CFB compound file.
///
/// Returns `None` if the parent chain is broken (circular reference or
/// missing parent), logging a warning so the caller can skip the entry.
fn build_entry_path(
    entry: &StorageEntry,
    vba_project_id: i32,
    id_map: &HashMap<i32, &StorageEntry>,
) -> Option<String> {
    let mut parts = vec![entry.name.clone()];
    let mut current_parent = entry.parent_id;
    let mut visited = HashSet::new();

    // Walk up the tree, stopping at VBAProject (which is the CFB root)
    while current_parent != vba_project_id {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn no_vba_project() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("should succeed");
        assert!(
            project.modules.is_empty(),
            "expected empty modules for database without VBA project"
        );
    }

    /// Helper to verify VBA project modules match expected names and types.
    fn assert_vba_modules(project: &VbaProject, expected: &[(&str, VbaModuleType)]) {
        assert_eq!(project.modules.len(), expected.len());
        let mut names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        let mut expected_names: Vec<&str> = expected.iter().map(|(n, _)| *n).collect();
        expected_names.sort();
        assert_eq!(names, expected_names);

        for (name, expected_type) in expected {
            let module = project.modules.iter().find(|m| m.name == *name).unwrap();
            assert_eq!(
                module.module_type, *expected_type,
                "type mismatch for {name}"
            );
            assert!(!module.source.is_empty(), "empty source for {name}");
        }
    }

    const EXPECTED_MODULES: &[(&str, VbaModuleType)] = &[
        ("Module1", VbaModuleType::Standard),
        ("Class1", VbaModuleType::ClassOrDocument),
        ("Form_Form1", VbaModuleType::ClassOrDocument),
    ];

    #[test]
    fn vba_v2003() {
        let path = skip_if_missing!("vbaV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    #[test]
    fn vba_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    #[test]
    fn storage_entries_v2003() {
        let path = skip_if_missing!("vbaV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let entries = read_storage_entries(&mut reader).unwrap();
        assert!(!entries.is_empty());
    }

    #[test]
    fn vba_v2000_catalog() {
        let path = skip_if_missing!("vbaV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = catalog::read_catalog(&mut reader).unwrap();
        assert!(!catalog.is_empty(), "catalog should not be empty");
    }

    #[test]
    fn vba_v2000() {
        // Uses MSysAccessObjects fallback (no MSysAccessStorage in this database).
        let path = skip_if_missing!("vbaV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    // -- build_entry_path unit tests ------------------------------------------

    fn make_entry(id: i32, parent_id: i32, name: &str) -> StorageEntry {
        StorageEntry {
            id,
            parent_id,
            name: name.to_string(),
            entry_type: 0,
            data: Vec::new(),
        }
    }

    #[test]
    fn build_entry_path_normal() {
        // VBAProject(id=1) -> VBA(id=2) -> dir(id=3)
        let vba = make_entry(2, 1, "VBA");
        let dir = make_entry(3, 2, "dir");
        let id_map: HashMap<i32, &StorageEntry> = [(2, &vba)].into_iter().collect();
        assert_eq!(
            build_entry_path(&dir, 1, &id_map),
            Some("/VBA/dir".to_string())
        );
    }

    #[test]
    fn build_entry_path_circular() {
        // A(id=2, parent=3) -> B(id=3, parent=2) — cycle
        let a = make_entry(2, 3, "A");
        let b = make_entry(3, 2, "B");
        let id_map: HashMap<i32, &StorageEntry> = [(2, &a), (3, &b)].into_iter().collect();
        assert_eq!(build_entry_path(&a, 1, &id_map), None);
    }

    #[test]
    fn build_entry_path_missing_parent() {
        // Entry with parent_id=99 which doesn't exist in the map
        let entry = make_entry(2, 99, "orphan");
        let id_map: HashMap<i32, &StorageEntry> = HashMap::new();
        assert_eq!(build_entry_path(&entry, 1, &id_map), None);
    }
}
