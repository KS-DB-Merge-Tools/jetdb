//! Page-level I/O, RC4 decryption, and database header parsing.

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::format::{db_header, row, FormatError, JetFormat, JetVersion};

// ---------------------------------------------------------------------------
// FileError
// ---------------------------------------------------------------------------

/// Errors that can occur when reading a Jet/ACE database file.
#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    Format(FormatError),
    FileTooSmall {
        expected: usize,
        actual: u64,
    },
    PageOutOfRange {
        page: u32,
        max_page: u32,
    },
    InvalidRow {
        page: u32,
        row: u16,
        reason: &'static str,
    },
    InvalidUsageMap {
        reason: &'static str,
    },
    InvalidTableDef {
        reason: &'static str,
    },
    InvalidProperty {
        reason: &'static str,
    },
    TableNotFound {
        name: String,
    },
    QueryNotFound {
        name: String,
    },
    ModuleNotFound {
        name: String,
    },
    InvalidVbaProject {
        reason: String,
    },
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Format(e) => write!(f, "format error: {e}"),
            Self::FileTooSmall { expected, actual } => {
                write!(
                    f,
                    "file too small: expected at least {expected} bytes, got {actual}"
                )
            }
            Self::PageOutOfRange { page, max_page } => {
                write!(f, "page {page} out of range (max page: {max_page})")
            }
            Self::InvalidRow { page, row, reason } => {
                write!(f, "invalid row {row} on page {page}: {reason}")
            }
            Self::InvalidUsageMap { reason } => {
                write!(f, "invalid usage map: {reason}")
            }
            Self::InvalidTableDef { reason } => {
                write!(f, "invalid table definition: {reason}")
            }
            Self::InvalidProperty { reason } => {
                write!(f, "invalid property data: {reason}")
            }
            Self::TableNotFound { name } => write!(f, "table not found: {name}"),
            Self::QueryNotFound { name } => write!(f, "query not found: {name}"),
            Self::ModuleNotFound { name } => write!(f, "VBA module not found: {name}"),
            Self::InvalidVbaProject { reason } => write!(f, "invalid VBA project: {reason}"),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Format(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<FormatError> for FileError {
    fn from(e: FormatError) -> Self {
        Self::Format(e)
    }
}

// ---------------------------------------------------------------------------
// RC4 implementation (private)
// ---------------------------------------------------------------------------

/// RC4 encrypt/decrypt in-place (KSA + PRGA).
fn rc4_transform(key: &[u8], buf: &mut [u8]) {
    // KSA
    let mut s: [u8; 256] = [0; 256];
    for (i, slot) in s.iter_mut().enumerate() {
        *slot = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0u8..=255 {
        let idx = i as usize;
        j = j.wrapping_add(s[idx]).wrapping_add(key[idx % key.len()]);
        s.swap(idx, j as usize);
    }

    // PRGA
    let mut i: u8 = 0;
    let mut j: u8 = 0;
    for byte in buf.iter_mut() {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        let k = s[(s[i as usize].wrapping_add(s[j as usize])) as usize];
        *byte ^= k;
    }
}

/// Fixed header encryption key used by Jet.
const HEADER_RC4_KEY: [u8; 4] = [0xC7, 0xDA, 0x39, 0x6B];

/// Decrypt the encrypted header region of page 0.
fn decrypt_header(page0: &mut [u8], version: JetVersion) {
    let enc_len = if version.is_jet3() { 126 } else { 128 };
    let end = db_header::ENCRYPTED_START + enc_len;
    if page0.len() >= end {
        rc4_transform(&HEADER_RC4_KEY, &mut page0[db_header::ENCRYPTED_START..end]);
    }
}

/// Decrypt a data page (page >= 1) if the database has an encryption key.
fn decrypt_page(buf: &mut [u8], db_key: u32, page: u32) {
    if db_key == 0 {
        return;
    }
    let page_key = (db_key ^ page).to_le_bytes();
    rc4_transform(&page_key, buf);
}

// ---------------------------------------------------------------------------
// DbHeader
// ---------------------------------------------------------------------------

/// Parsed database header information from page 0.
#[derive(Debug, Clone)]
pub struct DbHeader {
    pub version: JetVersion,
    pub format: &'static JetFormat,
    pub db_key: u32,
    pub lang_id: u16,
    pub code_page: u16,
}

// ---------------------------------------------------------------------------
// find_row — locate a row within a data page
// ---------------------------------------------------------------------------

/// Return the `(start, size)` of a row on a data page.
///
/// The row offset table begins at `data_row_count_pos + 2` in the page.
/// Each entry is 2 bytes LE; the flag bits are masked off with `row::OFFSET_MASK`.
/// For row 0 the upper bound is the page size; for row > 0 it is the previous
/// row's offset.
pub fn find_row(
    format: &JetFormat,
    page_data: &[u8],
    page: u32,
    row: u16,
) -> Result<(usize, usize), FileError> {
    let row_count_pos = format.data_row_count_pos;
    if page_data.len() < row_count_pos + 2 {
        return Err(FileError::InvalidRow {
            page,
            row,
            reason: "page too small for row count",
        });
    }
    let num_rows = u16::from_le_bytes([page_data[row_count_pos], page_data[row_count_pos + 1]]);
    if row >= num_rows {
        return Err(FileError::InvalidRow {
            page,
            row,
            reason: "row index exceeds row count",
        });
    }

    let table_start = row_count_pos + 2;

    // Read offset for the requested row
    let entry_pos = table_start + (row as usize) * 2;
    if entry_pos + 2 > page_data.len() {
        return Err(FileError::InvalidRow {
            page,
            row,
            reason: "row offset table overflow",
        });
    }
    let row_start =
        u16::from_le_bytes([page_data[entry_pos], page_data[entry_pos + 1]]) & row::OFFSET_MASK;

    let row_end = if row == 0 {
        format.page_size as u16
    } else {
        let prev_pos = table_start + ((row as usize) - 1) * 2;
        u16::from_le_bytes([page_data[prev_pos], page_data[prev_pos + 1]]) & row::OFFSET_MASK
    };

    if row_start >= row_end || row_end as usize > page_data.len() {
        return Err(FileError::InvalidRow {
            page,
            row,
            reason: "invalid row offsets",
        });
    }

    let start = row_start as usize;
    let size = (row_end - row_start) as usize;
    Ok((start, size))
}

// ---------------------------------------------------------------------------
// PageReader
// ---------------------------------------------------------------------------

/// Page-level reader for Jet/ACE database files.
///
/// Opens a database file, reads and decrypts the header, and provides
/// page-level random access.
pub struct PageReader {
    file: File,
    header: DbHeader,
    file_size: u64,
    page0_buf: Vec<u8>,
    page_buf: Vec<u8>,
    cached_page: Option<u32>,
}

impl PageReader {
    /// Open a database file and parse the header.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FileError> {
        let mut file = File::open(path.as_ref())?;
        let file_size = file.metadata()?.len();

        // We need at least the version byte at offset 0x14.
        const MIN_HEADER: u64 = (db_header::VERSION + 1) as u64;
        if file_size < MIN_HEADER {
            return Err(FileError::FileTooSmall {
                expected: MIN_HEADER as usize,
                actual: file_size,
            });
        }

        // Read version byte to determine page size.
        let mut ver_buf = [0u8; db_header::VERSION + 1];
        file.read_exact(&mut ver_buf)?;
        let version = JetVersion::from_byte(ver_buf[db_header::VERSION])?;
        let format = version.format();

        // File must be at least one page.
        if file_size < format.page_size as u64 {
            return Err(FileError::FileTooSmall {
                expected: format.page_size,
                actual: file_size,
            });
        }

        // Read full page 0.
        let mut page0_buf = vec![0u8; format.page_size];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut page0_buf)?;

        // Decrypt header region.
        decrypt_header(&mut page0_buf, version);

        // Extract header fields from decrypted page 0.
        let db_key = u32::from_le_bytes([
            page0_buf[db_header::DB_KEY],
            page0_buf[db_header::DB_KEY + 1],
            page0_buf[db_header::DB_KEY + 2],
            page0_buf[db_header::DB_KEY + 3],
        ]);

        let lang_id_offset = if version.is_jet3() {
            db_header::LANG_ID_JET3
        } else {
            db_header::LANG_ID_JET4
        };
        let lang_id =
            u16::from_le_bytes([page0_buf[lang_id_offset], page0_buf[lang_id_offset + 1]]);

        let code_page = u16::from_le_bytes([
            page0_buf[db_header::CODE_PAGE],
            page0_buf[db_header::CODE_PAGE + 1],
        ]);

        let header = DbHeader {
            version,
            format,
            db_key,
            lang_id,
            code_page,
        };

        let page_buf = vec![0u8; format.page_size];

        Ok(Self {
            file,
            header,
            file_size,
            page0_buf,
            page_buf,
            cached_page: None,
        })
    }

    /// Return the parsed database header.
    pub fn header(&self) -> &DbHeader {
        &self.header
    }

    /// Return the format constants for this database version.
    pub fn format(&self) -> &'static JetFormat {
        self.header.format
    }

    /// Return the total number of pages in the file.
    pub fn page_count(&self) -> u32 {
        (self.file_size / self.header.format.page_size as u64) as u32
    }

    /// Read and decrypt a page, returning a borrowed slice.
    ///
    /// Page 0 is always returned from the cached header buffer.
    /// Other pages are read on demand with a single-page cache.
    pub fn read_page(&mut self, page: u32) -> Result<&[u8], FileError> {
        if page == 0 {
            return Ok(&self.page0_buf);
        }

        if self.cached_page == Some(page) {
            return Ok(&self.page_buf);
        }

        let max_page = self.page_count().saturating_sub(1);
        if page > max_page {
            return Err(FileError::PageOutOfRange { page, max_page });
        }

        let page_size = self.header.format.page_size;
        let offset = page as u64 * page_size as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut self.page_buf)?;

        decrypt_page(&mut self.page_buf, self.header.db_key, page);

        self.cached_page = Some(page);
        Ok(&self.page_buf)
    }

    /// Read and decrypt a page, returning an owned copy of the data.
    ///
    /// Unlike `read_page`, the returned `Vec<u8>` is independent of `self`,
    /// so multiple pages can be held simultaneously.
    pub fn read_page_copy(&mut self, page: u32) -> Result<Vec<u8>, FileError> {
        self.read_page(page).map(|s| s.to_vec())
    }

    /// Read a row from a page using a pg_row pointer.
    ///
    /// A pg_row value encodes the page number in the upper 3 bytes and the
    /// row index in the lowest byte: `page = pg_row >> 8`, `row = pg_row & 0xFF`.
    pub fn read_pg_row(&mut self, pg_row: u32) -> Result<Vec<u8>, FileError> {
        let page_num = pg_row >> 8;
        let row_num = (pg_row & 0xFF) as u16;

        let page_data = self.read_page_copy(page_num)?;
        let (start, size) = find_row(self.header.format, &page_data, page_num, row_num)?;

        Ok(page_data[start..start + size].to_vec())
    }
}

impl fmt::Debug for PageReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageReader")
            .field("version", &self.header.version)
            .field("page_size", &self.header.format.page_size)
            .field("page_count", &self.page_count())
            .field("file_size", &self.file_size)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::JET4;

    // -- RC4 unit tests -------------------------------------------------------

    #[test]
    fn rc4_known_vector_key_01234() {
        // RFC 6229 test vector: Key = 0x0102030405
        let key: [u8; 5] = [0x01, 0x02, 0x03, 0x04, 0x05];
        let mut buf = [0u8; 16];
        rc4_transform(&key, &mut buf);
        // First 16 bytes of keystream from RFC 6229
        let expected: [u8; 16] = [
            0xB2, 0x39, 0x63, 0x05, 0xF0, 0x3D, 0xC0, 0x27, 0xCC, 0xC3, 0x52, 0x4A, 0x0A, 0x11,
            0x18, 0xA8,
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn rc4_roundtrip() {
        let key = b"test-key";
        let original = b"Hello, Jet database world!";
        let mut buf = *original;
        rc4_transform(key, &mut buf);
        assert_ne!(&buf, original); // encrypted != original
        rc4_transform(key, &mut buf);
        assert_eq!(&buf, original); // decrypted == original
    }

    #[test]
    fn rc4_empty_buffer() {
        let key = b"key";
        let mut buf = [];
        rc4_transform(key, &mut buf); // should not panic
    }

    #[test]
    fn header_rc4_keystream() {
        // The known RC4 keystream is generated from
        // key [0xC7, 0xDA, 0x39, 0x6B]. Verify our RC4 produces the same mask.
        let mut zeros = [0u8; 126];
        rc4_transform(&HEADER_RC4_KEY, &mut zeros);
        // First 8 bytes of the known RC4 keystream
        assert_eq!(zeros[0], 0xB5);
        assert_eq!(zeros[1], 0x6F);
        assert_eq!(zeros[2], 0x03);
        assert_eq!(zeros[3], 0x62);
        assert_eq!(zeros[4], 0x61);
        assert_eq!(zeros[5], 0x08);
        assert_eq!(zeros[6], 0xC2);
        assert_eq!(zeros[7], 0x55);
        // Last 2 bytes of the 126-byte mask (indices 124, 125)
        assert_eq!(zeros[124], 0xE9);
        assert_eq!(zeros[125], 0x2D);
    }

    // -- FileError tests ------------------------------------------------------

    #[test]
    fn file_error_display() {
        let e = FileError::FileTooSmall {
            expected: 4096,
            actual: 100,
        };
        assert_eq!(
            e.to_string(),
            "file too small: expected at least 4096 bytes, got 100"
        );

        let e = FileError::PageOutOfRange {
            page: 10,
            max_page: 5,
        };
        assert_eq!(e.to_string(), "page 10 out of range (max page: 5)");
    }

    #[test]
    fn file_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let fe: FileError = io_err.into();
        assert!(matches!(fe, FileError::Io(_)));
    }

    #[test]
    fn file_error_from_format() {
        let fmt_err = FormatError::UnknownVersion(0xFF);
        let fe: FileError = fmt_err.into();
        assert!(matches!(fe, FileError::Format(_)));
    }

    // -- Integration tests with real .mdb / .accdb files ----------------------

    /// Helper: resolve a test data path, returning None if the file doesn't exist.
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

    // -- Jet3 (V1997) ---------------------------------------------------------

    #[test]
    fn open_jet3_v1997() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let reader = PageReader::open(&path).expect("failed to open Jet3 file");
        assert_eq!(reader.header().version, JetVersion::Jet3);
        assert_eq!(reader.format().page_size, 2048);
        assert!(reader.page_count() > 0);
    }

    #[test]
    fn jet3_read_page1() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).expect("failed to open Jet3 file");
        if reader.page_count() > 1 {
            let page = reader.read_page(1).expect("failed to read page 1");
            assert_eq!(page.len(), 2048);
        }
    }

    // -- Jet4 (V2000/V2003) ---------------------------------------------------

    #[test]
    fn open_jet4_v2003() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let reader = PageReader::open(&path).expect("failed to open Jet4 file");
        assert_eq!(reader.header().version, JetVersion::Jet4);
        assert_eq!(reader.format().page_size, 4096);
        assert!(reader.page_count() > 0);
    }

    #[test]
    fn jet4_read_page1() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).expect("failed to open Jet4 file");
        if reader.page_count() > 1 {
            let page = reader.read_page(1).expect("failed to read page 1");
            assert_eq!(page.len(), 4096);
        }
    }

    // -- ACE12 (V2007) --------------------------------------------------------

    #[test]
    fn open_ace12_v2007() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let reader = PageReader::open(&path).expect("failed to open ACE12 file");
        assert_eq!(reader.header().version, JetVersion::Ace12);
        assert_eq!(reader.format().page_size, 4096);
        assert!(reader.page_count() > 0);
    }

    // -- ACE14 (V2010) --------------------------------------------------------

    #[test]
    fn open_ace14_v2010() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let reader = PageReader::open(&path).expect("failed to open ACE14 file");
        assert_eq!(reader.header().version, JetVersion::Ace14);
        assert_eq!(reader.format().page_size, 4096);
        assert!(reader.page_count() > 0);
    }

    // -- Edge cases -----------------------------------------------------------

    #[test]
    fn page_out_of_range() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).expect("failed to open file");
        let bad_page = reader.page_count() + 100;
        let err = reader.read_page(bad_page).unwrap_err();
        assert!(matches!(err, FileError::PageOutOfRange { .. }));
    }

    #[test]
    fn read_page0_returns_header_buffer() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).expect("failed to open file");
        let page0 = reader.read_page(0).expect("failed to read page 0");
        assert_eq!(page0.len(), 4096);
    }

    #[test]
    fn cached_page_consistency() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).expect("failed to open file");
        if reader.page_count() > 1 {
            let first = reader.read_page(1).expect("first read").to_vec();
            let second = reader.read_page(1).expect("second read").to_vec();
            assert_eq!(first, second, "cached page should return identical data");
        }
    }

    #[test]
    fn page_count_matches_file_size() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let reader = PageReader::open(&path).expect("failed to open file");
        let expected = reader.file_size / reader.format().page_size as u64;
        assert_eq!(reader.page_count(), expected as u32);
    }

    #[test]
    fn open_nonexistent_file() {
        let err = PageReader::open("/nonexistent/path/to/file.mdb").unwrap_err();
        assert!(matches!(err, FileError::Io(_)));
    }

    #[test]
    fn debug_impl() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let reader = PageReader::open(&path).expect("failed to open file");
        let debug = format!("{reader:?}");
        assert!(debug.contains("PageReader"));
        assert!(debug.contains("page_size"));
    }

    // -- find_row error paths -------------------------------------------------

    #[test]
    fn find_row_page_too_small() {
        let page_data = [0u8; 10]; // too small for JET4 row_count_pos (12)
        let result = find_row(&JET4, &page_data, 1, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            FileError::InvalidRow {
                reason: "page too small for row count",
                ..
            }
        ));
    }

    #[test]
    fn find_row_row_exceeds_count() {
        // JET4: row_count at offset 12. num_rows=1, request row=5
        let mut page_data = vec![0u8; 4096];
        page_data[12] = 1; // num_rows = 1
        page_data[13] = 0;
        let result = find_row(&JET4, &page_data, 1, 5);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            FileError::InvalidRow {
                reason: "row index exceeds row count",
                ..
            }
        ));
    }

    #[test]
    fn find_row_invalid_offsets() {
        // JET4: row_count at offset 12. num_rows=1, row=0
        // Row 0: end = page_size=4096, start must be < end
        // Set row offset to something >= 4096 (but within 13-bit mask)
        let mut page_data = vec![0u8; 4096];
        page_data[12] = 1; // num_rows = 1
        page_data[13] = 0;
        // offset table at 14: row 0 entry
        // Set start offset very high (but masked to 13 bits): 0x1FFF = 8191 → 8191 & 0x1FFF = 8191 > 4096
        page_data[14] = 0xFF;
        page_data[15] = 0x1F;
        let result = find_row(&JET4, &page_data, 1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn find_row_valid() {
        // Set up a valid single-row page for JET4
        let mut page_data = vec![0u8; 4096];
        page_data[12] = 1; // num_rows = 1
        page_data[13] = 0;
        // Row 0: offset table at 14. start=100 (stored as u16 LE)
        page_data[14] = 100;
        page_data[15] = 0;
        let result = find_row(&JET4, &page_data, 1, 0);
        assert!(result.is_ok());
        let (start, size) = result.unwrap();
        assert_eq!(start, 100);
        assert_eq!(size, 4096 - 100);
    }

    // -- FileError::Display remaining variants --------------------------------

    #[test]
    fn file_error_display_all_variants() {
        let e = FileError::InvalidUsageMap { reason: "test" };
        assert!(e.to_string().contains("test"));
        assert!(e.to_string().contains("invalid usage map"));

        let e = FileError::InvalidTableDef { reason: "bad tdef" };
        assert!(e.to_string().contains("bad tdef"));
        assert!(e.to_string().contains("invalid table definition"));

        let e = FileError::InvalidProperty { reason: "bad prop" };
        assert!(e.to_string().contains("bad prop"));
        assert!(e.to_string().contains("invalid property"));

        let e = FileError::TableNotFound { name: "T1".into() };
        assert!(e.to_string().contains("T1"));
        assert!(e.to_string().contains("table not found"));

        let e = FileError::QueryNotFound { name: "Q1".into() };
        assert!(e.to_string().contains("Q1"));
        assert!(e.to_string().contains("query not found"));

        let e = FileError::ModuleNotFound { name: "M1".into() };
        assert!(e.to_string().contains("M1"));
        assert!(e.to_string().contains("VBA module not found"));

        let e = FileError::InvalidVbaProject {
            reason: "corrupt".into(),
        };
        assert!(e.to_string().contains("corrupt"));
        assert!(e.to_string().contains("invalid VBA project"));

        let e = FileError::InvalidRow {
            page: 5,
            row: 3,
            reason: "oops",
        };
        assert!(e.to_string().contains("oops"));
        assert!(e.to_string().contains("invalid row"));
    }

    // -- Error::source() ------------------------------------------------------

    #[test]
    fn file_error_source() {
        use std::error::Error;

        let e = FileError::TableNotFound { name: "T".into() };
        assert!(e.source().is_none());

        let e = FileError::QueryNotFound { name: "Q".into() };
        assert!(e.source().is_none());

        let e = FileError::ModuleNotFound { name: "M".into() };
        assert!(e.source().is_none());

        let e = FileError::InvalidVbaProject { reason: "r".into() };
        assert!(e.source().is_none());

        let e = FileError::InvalidUsageMap { reason: "r" };
        assert!(e.source().is_none());

        let io_err = std::io::Error::other("io");
        let e = FileError::Io(io_err);
        assert!(e.source().is_some());

        let fmt_err = FormatError::InvalidEncoding;
        let e = FileError::Format(fmt_err);
        assert!(e.source().is_some());
    }

    // -- PageReader::open error paths -----------------------------------------

    #[test]
    fn open_empty_file() {
        let dir = std::env::temp_dir().join("jetdb_test_empty.mdb");
        std::fs::write(&dir, b"").unwrap();
        let err = PageReader::open(&dir).unwrap_err();
        assert!(matches!(err, FileError::FileTooSmall { .. }));
        std::fs::remove_file(&dir).ok();
    }

    #[test]
    fn open_too_small_file() {
        // File has valid version byte at 0x14 but is smaller than page size
        let mut data = vec![0u8; 0x15 + 1]; // just enough for version byte
        data[0x14] = 0x01; // Jet4 version
        let dir = std::env::temp_dir().join("jetdb_test_small.mdb");
        std::fs::write(&dir, &data).unwrap();
        let err = PageReader::open(&dir).unwrap_err();
        assert!(matches!(err, FileError::FileTooSmall { .. }));
        std::fs::remove_file(&dir).ok();
    }

    // -- decrypt_page symmetry test -------------------------------------------

    #[test]
    fn decrypt_page_roundtrip() {
        let db_key: u32 = 0x12345678;
        let page: u32 = 42;
        let original = vec![0xAA; 128];
        let mut buf = original.clone();
        decrypt_page(&mut buf, db_key, page);
        assert_ne!(buf, original); // encrypted != original
        decrypt_page(&mut buf, db_key, page); // RC4 is symmetric
        assert_eq!(buf, original);
    }

    #[test]
    fn decrypt_page_zero_key_noop() {
        let original = vec![0xBB; 64];
        let mut buf = original.clone();
        decrypt_page(&mut buf, 0, 1);
        assert_eq!(buf, original); // no change when db_key == 0
    }
}
