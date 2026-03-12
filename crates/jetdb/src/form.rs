//! Form and report design stream extraction from MSysAccessStorage.

use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::storage;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Form or report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormObjectType {
    Form,
    Report,
}

/// Which binary stream to extract from a form/report storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// Main design binary (layout, controls, properties, events).
    Blob,
    /// Control name and type list.
    TypeInfo,
    /// Small property metadata.
    PropData,
    /// Delta data (usually empty).
    BlobDelta,
}

impl StreamKind {
    /// The stream name as stored in MSysAccessStorage.
    fn storage_name(&self) -> &'static str {
        match self {
            Self::Blob => "Blob",
            Self::TypeInfo => "TypeInfo",
            Self::PropData => "PropData",
            Self::BlobDelta => "BlobDelta",
        }
    }
}

/// A form/report entry (for listing).
#[derive(Debug, Clone)]
pub struct FormEntry {
    pub name: String,
    pub object_type: FormObjectType,
}

/// Raw binary stream from a form/report.
#[derive(Debug, Clone)]
pub struct FormStream {
    pub name: String,
    pub object_type: FormObjectType,
    pub stream_kind: StreamKind,
    pub data: Vec<u8>,
}

/// A single control entry from TypeInfo.
#[derive(Debug, Clone)]
pub struct ControlInfo {
    pub name: String,
    pub type_code: u16,
    pub index: u32,
}

/// Parsed TypeInfo for a form/report.
#[derive(Debug, Clone)]
pub struct FormTypeInfo {
    pub form_name: String,
    pub object_type: FormObjectType,
    pub controls: Vec<ControlInfo>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List all form and report names in the database.
pub fn list_forms(reader: &mut PageReader) -> Result<Vec<FormEntry>, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let root_id = find_root_id(&entries);

    let mut result = Vec::new();

    // Collect forms
    if let Some(forms_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Forms" && storage::is_storage(e))
    {
        let dir = find_dir_data(&entries, forms_folder.id);
        if let Some(dir_data) = dir {
            let mapping = parse_dir_data(&dir_data.data)?;
            for (name, _storage_num) in mapping {
                result.push(FormEntry {
                    name,
                    object_type: FormObjectType::Form,
                });
            }
        }
    }

    // Collect reports
    if let Some(reports_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Reports" && storage::is_storage(e))
    {
        let dir = find_dir_data(&entries, reports_folder.id);
        if let Some(dir_data) = dir {
            let mapping = parse_dir_data(&dir_data.data)?;
            for (name, _storage_num) in mapping {
                result.push(FormEntry {
                    name,
                    object_type: FormObjectType::Report,
                });
            }
        }
    }

    Ok(result)
}

/// Read a raw binary stream from a named form or report.
pub fn read_form_stream(
    reader: &mut PageReader,
    name: &str,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, stream_data) = find_stream(&entries, name, stream_kind)?;

    Ok(FormStream {
        name: name.to_string(),
        object_type,
        stream_kind,
        data: stream_data,
    })
}

/// Read and parse TypeInfo for a named form or report.
pub fn read_form_type_info(
    reader: &mut PageReader,
    name: &str,
) -> Result<FormTypeInfo, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, stream_data) = find_stream(&entries, name, StreamKind::TypeInfo)?;
    let controls = parse_type_info(&stream_data)?;

    Ok(FormTypeInfo {
        form_name: name.to_string(),
        object_type,
        controls,
    })
}

// ---------------------------------------------------------------------------
// Internal: stream lookup
// ---------------------------------------------------------------------------

/// Find a specific stream for a named form/report.
///
/// Searches both Forms and Reports folders. Returns the object type and
/// the stream binary data.
fn find_stream(
    entries: &[storage::StorageEntry],
    name: &str,
    stream_kind: StreamKind,
) -> Result<(FormObjectType, Vec<u8>), FileError> {
    let root_id = find_root_id(entries);

    // Try Forms first, then Reports
    for (folder_name, obj_type) in [
        ("Forms", FormObjectType::Form),
        ("Reports", FormObjectType::Report),
    ] {
        if let Some(folder) = entries
            .iter()
            .find(|e| e.parent_id == root_id && e.name == folder_name && storage::is_storage(e))
        {
            if let Some(dir_data) = find_dir_data(entries, folder.id) {
                let mapping = parse_dir_data(&dir_data.data)?;
                if let Some((_form_name, storage_num)) =
                    mapping.iter().find(|(n, _)| n == name)
                {
                    // Find the storage entry with this number under the folder
                    if let Some(form_storage) = entries.iter().find(|e| {
                        e.parent_id == folder.id
                            && e.name == *storage_num
                            && storage::is_storage(e)
                    }) {
                        // Find the requested stream under this form storage
                        let stream_name = stream_kind.storage_name();
                        if let Some(stream_entry) = entries.iter().find(|e| {
                            e.parent_id == form_storage.id
                                && e.name == stream_name
                                && !storage::is_storage(e)
                        }) {
                            return Ok((obj_type, stream_entry.data.clone()));
                        }
                    }
                }
            }
        }
    }

    Err(FileError::FormNotFound {
        name: name.to_string(),
    })
}

/// Find the root entry ID (MSysAccessStorage_ROOT).
///
/// The root has `parent_id == id` (self-referencing) or is simply id=1.
fn find_root_id(entries: &[storage::StorageEntry]) -> i32 {
    entries
        .iter()
        .find(|e| e.parent_id == e.id && storage::is_storage(e))
        .map(|e| e.id)
        .unwrap_or(1)
}

/// Find the DirData stream entry under a folder.
///
/// The name may be prefixed with a control character (e.g., "\x03DirData").
fn find_dir_data(
    entries: &[storage::StorageEntry],
    folder_id: i32,
) -> Option<&storage::StorageEntry> {
    entries.iter().find(|e| {
        e.parent_id == folder_id
            && !storage::is_storage(e)
            && (e.name == "DirData" || e.name.ends_with("DirData"))
    })
}

// ---------------------------------------------------------------------------
// Internal: DirData parser
// ---------------------------------------------------------------------------

/// Parse DirData binary into (name, storage_number) pairs.
///
/// Format:
/// - 4-byte header (zeros)
/// - Entries: `[0x04] [len:u8] [UTF-16LE name] [storage_index:u16LE] [0x0000]`
///
/// `len` is normally reliable, but can be too short when names contain characters
/// whose UTF-16LE low byte is 0x00 (e.g., U+4E00 '一' → 00 4E). We use `len` as
/// the primary boundary but fall back to scanning if the payload doesn't end with
/// a null terminator.
fn parse_dir_data(data: &[u8]) -> Result<Vec<(String, String)>, FileError> {
    if data.len() < 4 {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut pos = 4; // skip header

    while pos + 1 < data.len() {
        if data[pos] != 0x04 {
            break;
        }
        let declared_len = data[pos + 1] as usize;
        pos += 2; // skip marker and len byte

        if declared_len < 4 || pos + declared_len > data.len() {
            break;
        }

        // Try declared_len first: check if payload ends with 0x0000
        let payload_end = pos + declared_len;
        let ends_with_null = payload_end >= 2
            && data[payload_end - 2] == 0x00
            && data[payload_end - 1] == 0x00;

        let actual_end = if ends_with_null {
            payload_end
        } else {
            // declared_len is wrong; scan forward for the null terminator.
            // Look for a u16-aligned 0x0000 that is followed by 0x04 or EOF.
            let mut scan = pos + declared_len;
            loop {
                if scan + 1 >= data.len() {
                    break scan + 1; // end of data
                }
                let val = u16::from_le_bytes([data[scan], data[scan + 1]]);
                if val == 0x0000 {
                    break scan + 2; // past the null terminator
                }
                scan += 2;
            }
        };

        // Payload layout: [name UTF-16LE] [storage_index u16LE] [0x0000]
        // Last 4 bytes: storage_index(2) + null(2)
        if actual_end < pos + 4 {
            pos = actual_end;
            continue;
        }

        let name_bytes = &data[pos..actual_end - 4];
        let storage_index =
            u16::from_le_bytes([data[actual_end - 4], data[actual_end - 3]]);

        if name_bytes.is_empty() {
            pos = actual_end;
            continue;
        }

        let name =
            encoding::decode_utf16le(name_bytes).map_err(|_| FileError::InvalidFormData {
                reason: "invalid UTF-16LE in DirData name",
            })?;

        entries.push((name, storage_index.to_string()));
        pos = actual_end;
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Internal: TypeInfo parser
// ---------------------------------------------------------------------------

/// TypeInfo magic number.
const TYPEINFO_MAGIC: u32 = 0xACCD_EAF7;

/// Parse TypeInfo binary into a list of control entries.
///
/// Format:
/// - Header (32 bytes): magic(u32) + field1(u32) + field2(i32) + count(u32) + GUID(16)
/// - Entries: ctrl_type(u16) + padding(u16) + index(u32) + name(Shift-JIS, NUL) + align(0x00)
fn parse_type_info(data: &[u8]) -> Result<Vec<ControlInfo>, FileError> {
    if data.len() < 32 {
        return Err(FileError::InvalidFormData {
            reason: "TypeInfo too short for header",
        });
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != TYPEINFO_MAGIC {
        return Err(FileError::InvalidFormData {
            reason: "TypeInfo magic mismatch",
        });
    }

    let entry_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    let mut controls = Vec::with_capacity(entry_count);
    let mut pos = 32; // skip header

    for _ in 0..entry_count {
        if pos + 8 > data.len() {
            break;
        }

        let ctrl_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        // skip padding u16 at pos+2..pos+4
        let index = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        pos += 8;

        // Read Shift-JIS NUL-terminated name
        let name_start = pos;
        while pos < data.len() && data[pos] != 0x00 {
            pos += 1;
        }

        let name_bytes = &data[name_start..pos];
        let name = decode_shift_jis(name_bytes);

        // Skip NUL terminator + alignment byte
        if pos < data.len() {
            pos += 1; // NUL terminator
        }
        if pos < data.len() && data[pos] == 0x00 {
            pos += 1; // alignment NUL
        }

        controls.push(ControlInfo {
            name,
            type_code: ctrl_type,
            index,
        });
    }

    Ok(controls)
}

/// Decode Shift-JIS (cp932) bytes to a String.
fn decode_shift_jis(bytes: &[u8]) -> String {
    let (result, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    result.into_owned()
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

    // -- DirData parser unit tests -------------------------------------------

    #[test]
    fn parse_dir_data_empty() {
        // Just a 4-byte header
        let data = [0x00, 0x00, 0x00, 0x00];
        let result = parse_dir_data(&data).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_dir_data_too_short() {
        let result = parse_dir_data(&[0x00]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_dir_data_single_entry() {
        // Header + one entry: name "AB" (UTF-16LE) + storage_index=5 + null
        let mut data = vec![0x00, 0x00, 0x00, 0x00]; // header
        data.push(0x04); // marker
        data.push(0x08); // len = 4 (name) + 2 (index) + 2 (null)
        // name "AB" in UTF-16LE
        data.extend_from_slice(&[0x41, 0x00, 0x42, 0x00]);
        // storage_index = 5
        data.extend_from_slice(&[0x05, 0x00]);
        // null terminator
        data.extend_from_slice(&[0x00, 0x00]);

        let result = parse_dir_data(&data).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "AB");
        assert_eq!(result[0].1, "5");
    }

    // -- TypeInfo parser unit tests ------------------------------------------

    #[test]
    fn parse_type_info_too_short() {
        let data = [0u8; 16];
        let result = parse_type_info(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_bad_magic() {
        let mut data = [0u8; 32];
        data[0] = 0xFF; // wrong magic
        let result = parse_type_info(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_empty_entries() {
        let mut data = vec![0u8; 32];
        // Set magic
        data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
        // entry_count = 0
        data[8..12].copy_from_slice(&0u32.to_le_bytes());

        let result = parse_type_info(&data).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_type_info_single_entry() {
        let mut data = vec![0u8; 32];
        // Set magic
        data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
        // field2 = -1
        data[8..12].copy_from_slice(&1u32.to_le_bytes()); // entry_count = 1

        // Entry: ctrl_type=0x0B68, padding=0, index=0, name="Btn1\0\0"
        data.extend_from_slice(&[0x68, 0x0B]); // ctrl_type
        data.extend_from_slice(&[0x00, 0x00]); // padding
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // index
        data.extend_from_slice(b"Btn1"); // ASCII name (valid Shift-JIS)
        data.push(0x00); // NUL terminator
        data.push(0x00); // alignment

        let result = parse_type_info(&data).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Btn1");
        assert_eq!(result[0].type_code, 0x0B68);
        assert_eq!(result[0].index, 0);
    }

    // -- Integration tests with real files -----------------------------------

    #[test]
    fn list_forms_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        // vbaV2007.accdb has Form1
        assert!(
            forms.iter().any(|e| e.name == "Form1"),
            "expected Form1 in form list, got: {:?}",
            forms.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn read_form_blob_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let stream = read_form_stream(&mut reader, "Form1", StreamKind::Blob).unwrap();
        assert!(!stream.data.is_empty(), "Blob should not be empty");
        assert_eq!(stream.object_type, FormObjectType::Form);
    }

    #[test]
    fn read_form_type_info_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let type_info = read_form_type_info(&mut reader, "Form1").unwrap();
        assert!(
            !type_info.controls.is_empty(),
            "TypeInfo should have at least one control"
        );
        assert_eq!(type_info.object_type, FormObjectType::Form);
    }

    #[test]
    fn form_not_found() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let result = read_form_stream(&mut reader, "NoSuchForm", StreamKind::Blob);
        assert!(matches!(result, Err(FileError::FormNotFound { .. })));
    }

    #[test]
    fn no_forms_in_plain_db() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        assert!(forms.is_empty(), "expected no forms in plain test database");
    }
}
