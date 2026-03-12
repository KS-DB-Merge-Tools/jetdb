//! Form and report design stream extraction from MSysAccessStorage.
//!
//! Also provides Blob binary parsing to extract form/report properties
//! (RecordSource, ControlSource, Filter, etc.) and per-control properties.

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

/// Property value extracted from a Blob stream.
#[derive(Debug, Clone)]
pub enum BlobValue {
    Bool(bool),
    Short(i16),
    Long(i32),
    Color(u32),
    Double(f64),
    Guid(String),
    Text(String),
    Binary(Vec<u8>),
}

/// A single property entry from the Blob binary.
#[derive(Debug, Clone)]
pub struct BlobProperty {
    pub prop_id: u16,
    pub value: BlobValue,
}

/// Properties for a single control, extracted from the Blob.
#[derive(Debug, Clone)]
pub struct ControlProperties {
    /// Control name (from Name property 0x14 in Blob).
    pub name: String,
    /// Control type code (from TypeInfo).
    pub type_code: u16,
    /// Properties for this control.
    pub properties: Vec<BlobProperty>,
}

/// All properties for a form or report, including per-control properties.
#[derive(Debug, Clone)]
pub struct FormProperties {
    pub form_name: String,
    pub object_type: FormObjectType,
    /// Form/report-level properties (RecordSource, Filter, etc.).
    pub properties: Vec<BlobProperty>,
    /// Per-control properties.
    pub controls: Vec<ControlProperties>,
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

/// Return the known property name for a given prop_id, or `None`.
pub fn prop_id_name(prop_id: u16) -> Option<&'static str> {
    match prop_id {
        0x0011 => Some("Caption"),
        0x0012 => Some("ColumnWidths"),
        0x0014 => Some("Name"),
        0x001B => Some("ControlSource"),
        0x0022 => Some("FontName"),
        0x0026 => Some("Format"),
        0x005B => Some("RowSource"),
        0x005D => Some("RowSourceType"),
        0x0068 => Some("OnClick"),
        0x0072 => Some("OnDblClick"),
        0x0074 => Some("OnMouseDown"),
        0x007E => Some("OnKeyPress"),
        0x009C => Some("RecordSource"),
        0x00A0 => Some("FontName"),
        0x00F5 => Some("Filter"),
        0x010A => Some("LabelType"),
        0x015A => Some("InputMask"),
        _ => None,
    }
}

impl BlobProperty {
    /// Return the known property name, or `None` for unknown IDs.
    pub fn name(&self) -> Option<&'static str> {
        prop_id_name(self.prop_id)
    }

    /// Return a display label: the known name or `0xXXXX`.
    pub fn display_name(&self) -> String {
        match self.name() {
            Some(n) => n.to_string(),
            None => format!("0x{:04X}", self.prop_id),
        }
    }
}

impl std::fmt::Display for BlobValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", if *v { "yes" } else { "no" }),
            Self::Short(v) => write!(f, "{v}"),
            Self::Long(v) => write!(f, "{v}"),
            Self::Color(v) => write!(f, "#{:06X}", v & 0x00FF_FFFF),
            Self::Double(v) => write!(f, "{v}"),
            Self::Guid(v) => write!(f, "{v}"),
            Self::Text(v) => write!(f, "{v}"),
            Self::Binary(v) => write!(f, "({} bytes)", v.len()),
        }
    }
}

/// Read and parse all properties from a named form or report.
///
/// Parses the Blob binary to extract form-level and per-control properties.
/// TypeInfo is used to associate control names and type codes.
pub fn read_form_properties(
    reader: &mut PageReader,
    name: &str,
) -> Result<FormProperties, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, blob_data) = find_stream(&entries, name, StreamKind::Blob)?;

    // Try to get TypeInfo for control name/type mapping; not fatal if missing.
    let type_info_controls = match find_stream(&entries, name, StreamKind::TypeInfo) {
        Ok((_, ti_data)) => parse_type_info(&ti_data).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let (form_props, control_prop_groups) = parse_blob(&blob_data)?;

    // Merge control property groups with TypeInfo data.
    let controls = control_prop_groups
        .into_iter()
        .enumerate()
        .map(|(i, props)| {
            // Find control name from Blob's Name property (0x14).
            let blob_name = props
                .iter()
                .find(|p| p.prop_id == 0x0014)
                .and_then(|p| match &p.value {
                    BlobValue::Text(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| format!("Control_{i}"));

            // Match with TypeInfo by index to get type_code.
            let type_code = type_info_controls.get(i).map(|c| c.type_code).unwrap_or(0);

            ControlProperties {
                name: blob_name,
                type_code,
                properties: props,
            }
        })
        .collect();

    Ok(FormProperties {
        form_name: name.to_string(),
        object_type,
        properties: form_props,
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
// Internal: Blob parser
// ---------------------------------------------------------------------------

/// Parse the Blob binary into form-level and per-control property groups.
///
/// Returns (form_properties, vec_of_control_properties).
/// Each control section starts with a Name (0x14) property.
/// Parsing stops gracefully on unknown types or malformed data.
///
/// Entry format (reverse-engineered):
///   prop_id(u16) + type(u32) + B(u32) + C(u32) + data[type-dependent]
///   Header = 14 bytes, then type-specific data.
///
/// Some types (≥ 0x08) have a 4-byte trailer after the data.
fn parse_blob(data: &[u8]) -> Result<(Vec<BlobProperty>, Vec<Vec<BlobProperty>>), FileError> {
    if data.len() < 14 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Skip 8-byte blob header + 6-byte section preamble.
    let mut pos = 14;
    let mut all_props = Vec::new();

    while pos + 14 <= data.len() {
        let prop_id = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let type_code = u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]);
        let _b = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]);
        let c = u32::from_le_bytes([data[pos + 10], data[pos + 11], data[pos + 12], data[pos + 13]]);
        let data_start = pos + 14;

        match type_code {
            0x01 => {
                // Bool: 4 bytes data, no trailer. Total = 18.
                if data_start + 4 > data.len() {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Bool(val != 0),
                });
                pos += 18;
            }
            0x02 => {
                // Short: 5 bytes data, no trailer. Total = 19.
                if data_start + 5 > data.len() {
                    break;
                }
                let val = i16::from_le_bytes([data[data_start], data[data_start + 1]]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Short(val),
                });
                pos += 19;
            }
            0x03 => {
                // Long: 6 bytes data, no trailer. Total = 20.
                if data_start + 6 > data.len() {
                    break;
                }
                let val = i32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Long(val),
                });
                pos += 20;
            }
            0x04 => {
                // Color: 8 bytes data (4 color + 4 extra), no trailer. Total = 22.
                if data_start + 8 > data.len() {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Color(val),
                });
                pos += 22;
            }
            0x08 => {
                // Double: 8 bytes data + 4 byte trailer. Total = 26.
                if data_start + 12 > data.len() {
                    break;
                }
                let val = f64::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                    data[data_start + 4], data[data_start + 5],
                    data[data_start + 6], data[data_start + 7],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Double(val),
                });
                pos += 26;
            }
            0x09 => {
                // GUID: 16 bytes data + 4 byte trailer. Total = 34.
                if data_start + 20 > data.len() {
                    break;
                }
                let guid = format_guid(&data[data_start..data_start + 16]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Guid(guid),
                });
                pos += 34;
            }
            0x0A | 0x0C => {
                // Variable-length text: C bytes data + 4 byte trailer.
                let byte_len = c as usize;
                if data_start + byte_len + 4 > data.len() {
                    break;
                }
                let text_bytes = &data[data_start..data_start + byte_len];
                let text = encoding::decode_utf16le(text_bytes).unwrap_or_else(|_| {
                    String::from_utf8_lossy(text_bytes).into_owned()
                });
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Text(text),
                });
                pos += 14 + byte_len + 4;
            }
            0x0B => {
                // Variable-length binary: C bytes data + 4 byte trailer.
                let byte_len = c as usize;
                if data_start + byte_len + 4 > data.len() {
                    break;
                }
                let bin_data = data[data_start..data_start + byte_len].to_vec();
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Binary(bin_data),
                });
                pos += 14 + byte_len + 4;
            }
            _ => {
                // Unknown type — stop parsing gracefully.
                break;
            }
        }
    }

    // The sequential parse above covers form-level properties but stops at binary
    // layout data in the middle. Control sections come later, each starting with a
    // Name (prop_id=0x14, type=0x0A) property. Scan the remaining blob for these.
    let control_groups = scan_control_sections(data, pos);

    Ok((all_props, control_groups))
}

/// Scan the blob for control property sections starting with Name (0x14) entries.
///
/// Each control section begins with prop_id=0x14 (Name) + type=0x0A (Text).
/// The byte pattern is `[14, 00, 0A, 00, 00, 00]`.
fn scan_control_sections(data: &[u8], start: usize) -> Vec<Vec<BlobProperty>> {
    // Pattern: prop_id(0x14, 0x00) + type(0x0A, 0x00, 0x00, 0x00)
    let pattern: [u8; 6] = [0x14, 0x00, 0x0A, 0x00, 0x00, 0x00];

    // Find all positions where control sections start.
    let mut section_starts = Vec::new();
    let mut search_pos = start;
    while search_pos + 6 <= data.len() {
        if data[search_pos..search_pos + 6] == pattern {
            section_starts.push(search_pos);
            search_pos += 6; // skip past this match
        } else {
            search_pos += 1;
        }
    }

    // Parse properties from each section start.
    let mut control_groups = Vec::new();
    for (i, &sec_start) in section_starts.iter().enumerate() {
        let sec_end = section_starts.get(i + 1).copied().unwrap_or(data.len());
        let props = parse_section_props(data, sec_start, sec_end);
        if !props.is_empty() {
            control_groups.push(props);
        }
    }

    control_groups
}

/// Parse property entries from a section of the blob.
///
/// Same entry format as `parse_blob`, but limited to the given range.
fn parse_section_props(data: &[u8], start: usize, end: usize) -> Vec<BlobProperty> {
    let mut props = Vec::new();
    let mut pos = start;

    while pos + 14 <= end {
        let prop_id = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let type_code = u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]);
        let _b = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]);
        let c = u32::from_le_bytes([data[pos + 10], data[pos + 11], data[pos + 12], data[pos + 13]]);
        let data_start = pos + 14;

        match type_code {
            0x01 => {
                if data_start + 4 > end { break; }
                let val = u32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                props.push(BlobProperty { prop_id, value: BlobValue::Bool(val != 0) });
                pos += 18;
            }
            0x02 => {
                if data_start + 5 > end { break; }
                let val = i16::from_le_bytes([data[data_start], data[data_start + 1]]);
                props.push(BlobProperty { prop_id, value: BlobValue::Short(val) });
                pos += 19;
            }
            0x03 => {
                if data_start + 6 > end { break; }
                let val = i32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                props.push(BlobProperty { prop_id, value: BlobValue::Long(val) });
                pos += 20;
            }
            0x04 => {
                if data_start + 8 > end { break; }
                let val = u32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                props.push(BlobProperty { prop_id, value: BlobValue::Color(val) });
                pos += 22;
            }
            0x06 => {
                // Type 6: observed in control sections. 6 bytes data, no trailer. Total = 20.
                if data_start + 6 > end { break; }
                let val = i32::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                ]);
                props.push(BlobProperty { prop_id, value: BlobValue::Long(val) });
                pos += 20;
            }
            0x08 => {
                if data_start + 12 > end { break; }
                let val = f64::from_le_bytes([
                    data[data_start], data[data_start + 1],
                    data[data_start + 2], data[data_start + 3],
                    data[data_start + 4], data[data_start + 5],
                    data[data_start + 6], data[data_start + 7],
                ]);
                props.push(BlobProperty { prop_id, value: BlobValue::Double(val) });
                pos += 26;
            }
            0x09 => {
                if data_start + 20 > end { break; }
                let guid = format_guid(&data[data_start..data_start + 16]);
                props.push(BlobProperty { prop_id, value: BlobValue::Guid(guid) });
                pos += 34;
            }
            0x0A | 0x0C => {
                let byte_len = c as usize;
                if data_start + byte_len + 4 > end { break; }
                let text_bytes = &data[data_start..data_start + byte_len];
                let text = encoding::decode_utf16le(text_bytes).unwrap_or_else(|_| {
                    String::from_utf8_lossy(text_bytes).into_owned()
                });
                props.push(BlobProperty { prop_id, value: BlobValue::Text(text) });
                pos += 14 + byte_len + 4;
            }
            0x0B => {
                let byte_len = c as usize;
                if data_start + byte_len + 4 > end { break; }
                let bin_data = data[data_start..data_start + byte_len].to_vec();
                props.push(BlobProperty { prop_id, value: BlobValue::Binary(bin_data) });
                pos += 14 + byte_len + 4;
            }
            _ => {
                break;
            }
        }
    }

    props
}

/// Format 16 bytes as a GUID string `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`.
fn format_guid(bytes: &[u8]) -> String {
    if bytes.len() < 16 {
        return format!("({} bytes)", bytes.len());
    }
    let d1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let d2 = u16::from_le_bytes([bytes[4], bytes[5]]);
    let d3 = u16::from_le_bytes([bytes[6], bytes[7]]);
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        d1, d2, d3, bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    )
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

    // -- Blob parser unit tests (synthetic binary) ----------------------------

    /// Build a minimal blob with 8-byte header + 6-byte preamble + entries.
    fn make_blob(entries: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 14]; // 8 header + 6 preamble
        data.extend_from_slice(entries);
        data
    }

    /// Build a single Blob entry for a given type.
    fn make_entry(prop_id: u16, type_code: u32, b: u32, c: u32, payload: &[u8]) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend_from_slice(&prop_id.to_le_bytes());
        entry.extend_from_slice(&type_code.to_le_bytes());
        entry.extend_from_slice(&b.to_le_bytes());
        entry.extend_from_slice(&c.to_le_bytes());
        entry.extend_from_slice(payload);
        entry
    }

    #[test]
    fn parse_blob_empty() {
        let data = vec![0u8; 14];
        let (form_props, controls) = parse_blob(&data).unwrap();
        assert!(form_props.is_empty());
        assert!(controls.is_empty());
    }

    #[test]
    fn parse_blob_too_short() {
        let (form_props, controls) = parse_blob(&[0u8; 5]).unwrap();
        assert!(form_props.is_empty());
        assert!(controls.is_empty());
    }

    #[test]
    fn parse_blob_bool_entry() {
        // Bool (type 0x01): 4 bytes data, total entry = 18
        let payload = [0x01, 0x00, 0x00, 0x00]; // true
        let entry = make_entry(0x0013, 0x01, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x0013);
        assert!(matches!(props[0].value, BlobValue::Bool(true)));
    }

    #[test]
    fn parse_blob_short_entry() {
        // Short (type 0x02): 5 bytes data, total entry = 19
        let payload = [0x2A, 0x00, 0x00, 0x00, 0x00]; // 42
        let entry = make_entry(0x0098, 0x02, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert!(matches!(props[0].value, BlobValue::Short(42)));
    }

    #[test]
    fn parse_blob_long_entry() {
        // Long (type 0x03): 6 bytes data, total entry = 20
        let payload = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00]; // 256
        let entry = make_entry(0x002A, 0x03, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert!(matches!(props[0].value, BlobValue::Long(256)));
    }

    #[test]
    fn parse_blob_text_entry() {
        // Text (type 0x0A): C bytes data + 4 byte trailer
        let text = "AB"; // UTF-16LE: [0x41, 0x00, 0x42, 0x00]
        let text_bytes = [0x41, 0x00, 0x42, 0x00];
        let c = text_bytes.len() as u32;
        let mut payload = Vec::new();
        payload.extend_from_slice(&text_bytes);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x009C, 0x0A, 0, c, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x009C); // RecordSource
        match &props[0].value {
            BlobValue::Text(s) => assert_eq!(s, text),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_blob_binary_entry() {
        // Binary (type 0x0B): C bytes data + 4 byte trailer
        let bin = [0xDE, 0xAD, 0xBE, 0xEF];
        let c = bin.len() as u32;
        let mut payload = Vec::new();
        payload.extend_from_slice(&bin);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x00BD, 0x0B, 0, c, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        match &props[0].value {
            BlobValue::Binary(v) => assert_eq!(v.as_slice(), &bin),
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_blob_multiple_entries() {
        let mut entries = Vec::new();
        // Bool entry
        entries.extend_from_slice(&make_entry(0x0013, 0x01, 0, 0, &[0x00; 4]));
        // Short entry
        entries.extend_from_slice(&make_entry(0x0098, 0x02, 0, 0, &[0x07, 0x00, 0x00, 0x00, 0x00]));
        // Long entry
        entries.extend_from_slice(&make_entry(0x002A, 0x03, 0, 0, &[0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00]));

        let blob = make_blob(&entries);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 3);
        assert!(matches!(props[0].value, BlobValue::Bool(false)));
        assert!(matches!(props[1].value, BlobValue::Short(7)));
        assert!(matches!(props[2].value, BlobValue::Long(-1)));
    }

    #[test]
    fn parse_blob_stops_on_unknown_type() {
        let mut entries = Vec::new();
        // Valid Bool entry
        entries.extend_from_slice(&make_entry(0x0013, 0x01, 0, 0, &[0x01; 4]));
        // Unknown type 0xFF
        entries.extend_from_slice(&make_entry(0x9999, 0xFF, 0, 0, &[0x00; 10]));

        let blob = make_blob(&entries);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1, "should stop at unknown type");
    }

    #[test]
    fn parse_blob_guid_entry() {
        // GUID (type 0x09): 16 bytes data + 4 byte trailer, total = 34
        let guid_bytes = [
            0x50, 0xA6, 0x64, 0x8D, 0xE7, 0x62, 0x03, 0x49,
            0x97, 0x33, 0x0D, 0x8C, 0xE8, 0x49, 0x78, 0xBF,
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&guid_bytes);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x0178, 0x09, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        match &props[0].value {
            BlobValue::Guid(s) => assert!(s.starts_with('{') && s.ends_with('}')),
            other => panic!("expected Guid, got {:?}", other),
        }
    }

    #[test]
    fn prop_id_name_known() {
        assert_eq!(prop_id_name(0x009C), Some("RecordSource"));
        assert_eq!(prop_id_name(0x001B), Some("ControlSource"));
        assert_eq!(prop_id_name(0x00F5), Some("Filter"));
        assert_eq!(prop_id_name(0x0014), Some("Name"));
    }

    #[test]
    fn prop_id_name_unknown() {
        assert_eq!(prop_id_name(0xFFFF), None);
    }

    #[test]
    fn blob_property_display_name() {
        let known = BlobProperty { prop_id: 0x009C, value: BlobValue::Bool(true) };
        assert_eq!(known.display_name(), "RecordSource");

        let unknown = BlobProperty { prop_id: 0x1234, value: BlobValue::Bool(true) };
        assert_eq!(unknown.display_name(), "0x1234");
    }

    #[test]
    fn blob_value_display() {
        assert_eq!(format!("{}", BlobValue::Bool(true)), "yes");
        assert_eq!(format!("{}", BlobValue::Bool(false)), "no");
        assert_eq!(format!("{}", BlobValue::Short(42)), "42");
        assert_eq!(format!("{}", BlobValue::Long(-1)), "-1");
        assert_eq!(format!("{}", BlobValue::Color(0x00FF0000)), "#FF0000");
        assert_eq!(format!("{}", BlobValue::Text("hello".into())), "hello");
        assert_eq!(format!("{}", BlobValue::Binary(vec![0; 10])), "(10 bytes)");
    }

    // -- Integration test: read_form_properties with real file ----------------

    #[test]
    fn read_form_properties_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "Form1").unwrap();
        assert_eq!(props.object_type, FormObjectType::Form);
        // Should have at least some form-level properties.
        assert!(
            !props.properties.is_empty(),
            "expected form-level properties, got empty"
        );
    }

    // TODO: Integration tests with formPropTest.accdb (pending test data creation):
    // - Verify RecordSource, Filter, ControlSource values
    // - Verify report properties
    // - Verify control property parsing with calculated fields
}
