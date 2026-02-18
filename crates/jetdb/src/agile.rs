//! Agile Encryption (MS-OFFCRYPTO) support for .accdb files.
//!
//! Access 2007+ uses Agile Encryption when a database password is set.
//! The EncryptionInfo XML is stored at page 0 offset 0x299.

use crate::file::FileError;
use crate::format::db_header;

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

// ---------------------------------------------------------------------------
// Block key constants for password key encryptor
// ---------------------------------------------------------------------------

const VERIFIER_HASH_INPUT_BLOCK_KEY: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_HASH_VALUE_BLOCK_KEY: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const ENCRYPTED_KEY_VALUE_BLOCK_KEY: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];

/// Padding byte for key derivation and IV construction (MS-OFFCRYPTO §2.3.6.2).
const HASH_PAD_BYTE: u8 = 0x36;

// ---------------------------------------------------------------------------
// AgileParams
// ---------------------------------------------------------------------------

/// Parameters parsed from the EncryptionInfo XML.
#[derive(Debug, Clone)]
pub(crate) struct AgileParams {
    // Key data (for page decryption)
    pub key_bits: usize,
    pub block_size: usize,
    pub hash_algorithm: HashAlgorithm,
    pub salt_value: Vec<u8>,

    // Password key encryptor
    pub pe_spin_count: u32,
    pub pe_salt_value: Vec<u8>,
    pub pe_hash_algorithm: HashAlgorithm,
    pub pe_key_bits: usize,
    pub pe_block_size: usize,
    pub encrypted_verifier_hash_input: Vec<u8>,
    pub encrypted_verifier_hash_value: Vec<u8>,
    pub encrypted_key_value: Vec<u8>,
}

/// Supported hash algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    fn from_str(s: &str) -> Result<Self, FileError> {
        match s {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            "SHA384" => Ok(Self::Sha384),
            "SHA512" => Ok(Self::Sha512),
            _ => Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported hash algorithm: {s}"),
            }),
        }
    }

    fn hash_size(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

// ---------------------------------------------------------------------------
// Generic hash helper
// ---------------------------------------------------------------------------

fn hash_with<D: Digest>(data: &[u8]) -> Vec<u8> {
    let mut h = D::new();
    h.update(data);
    h.finalize().to_vec()
}

fn hash_bytes(algo: HashAlgorithm, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgorithm::Sha1 => hash_with::<Sha1>(data),
        HashAlgorithm::Sha256 => hash_with::<Sha256>(data),
        HashAlgorithm::Sha384 => hash_with::<Sha384>(data),
        HashAlgorithm::Sha512 => hash_with::<Sha512>(data),
    }
}

// ---------------------------------------------------------------------------
// XML parsing with quick-xml
// ---------------------------------------------------------------------------

/// Parse an attribute value from a quick-xml `BytesStart` element.
fn get_attr(
    e: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, FileError> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| FileError::UnsupportedEncryption {
            reason: format!("invalid XML attribute: {err}"),
        })?;
        if attr.key.as_ref() == name {
            let val = attr.unescape_value().map_err(|err| FileError::UnsupportedEncryption {
                reason: format!("invalid XML attribute value: {err}"),
            })?;
            return Ok(Some(val.into_owned()));
        }
    }
    Ok(None)
}

fn require_attr(
    e: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
    tag_name: &str,
) -> Result<String, FileError> {
    get_attr(e, name)?.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: format!(
            "missing {}.{} in EncryptionInfo",
            tag_name,
            String::from_utf8_lossy(name)
        ),
    })
}

fn parse_usize(val: &str, tag: &str, attr: &str) -> Result<usize, FileError> {
    val.parse().map_err(|_| FileError::UnsupportedEncryption {
        reason: format!("invalid {tag}.{attr} value: {val}"),
    })
}

fn parse_u32(val: &str, tag: &str, attr: &str) -> Result<u32, FileError> {
    val.parse().map_err(|_| FileError::UnsupportedEncryption {
        reason: format!("invalid {tag}.{attr} value: {val}"),
    })
}

fn parse_base64(val: &str, tag: &str, attr: &str) -> Result<Vec<u8>, FileError> {
    BASE64.decode(val).map_err(|_| FileError::UnsupportedEncryption {
        reason: format!("invalid base64 in {tag}.{attr}"),
    })
}

/// Data extracted from the `<keyData>` element.
struct KeyDataAttrs {
    key_bits: usize,
    block_size: usize,
    hash_algorithm: HashAlgorithm,
    salt_value: Vec<u8>,
}

/// Data extracted from the `<*:encryptedKey>` element.
struct EncryptedKeyAttrs {
    spin_count: u32,
    salt_value: Vec<u8>,
    hash_algorithm: HashAlgorithm,
    key_bits: usize,
    block_size: usize,
    encrypted_verifier_hash_input: Vec<u8>,
    encrypted_verifier_hash_value: Vec<u8>,
    encrypted_key_value: Vec<u8>,
}

fn parse_key_data(e: &quick_xml::events::BytesStart<'_>) -> Result<KeyDataAttrs, FileError> {
    let tag = "keyData";
    Ok(KeyDataAttrs {
        key_bits: parse_usize(&require_attr(e, b"keyBits", tag)?, tag, "keyBits")?,
        block_size: parse_usize(&require_attr(e, b"blockSize", tag)?, tag, "blockSize")?,
        hash_algorithm: HashAlgorithm::from_str(&require_attr(e, b"hashAlgorithm", tag)?)?,
        salt_value: parse_base64(&require_attr(e, b"saltValue", tag)?, tag, "saltValue")?,
    })
}

fn parse_encrypted_key(
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<EncryptedKeyAttrs, FileError> {
    let tag = "encryptedKey";
    Ok(EncryptedKeyAttrs {
        spin_count: parse_u32(&require_attr(e, b"spinCount", tag)?, tag, "spinCount")?,
        salt_value: parse_base64(&require_attr(e, b"saltValue", tag)?, tag, "saltValue")?,
        hash_algorithm: HashAlgorithm::from_str(&require_attr(e, b"hashAlgorithm", tag)?)?,
        key_bits: parse_usize(&require_attr(e, b"keyBits", tag)?, tag, "keyBits")?,
        block_size: parse_usize(&require_attr(e, b"blockSize", tag)?, tag, "blockSize")?,
        encrypted_verifier_hash_input: parse_base64(
            &require_attr(e, b"encryptedVerifierHashInput", tag)?,
            tag,
            "encryptedVerifierHashInput",
        )?,
        encrypted_verifier_hash_value: parse_base64(
            &require_attr(e, b"encryptedVerifierHashValue", tag)?,
            tag,
            "encryptedVerifierHashValue",
        )?,
        encrypted_key_value: parse_base64(
            &require_attr(e, b"encryptedKeyValue", tag)?,
            tag,
            "encryptedKeyValue",
        )?,
    })
}

// ---------------------------------------------------------------------------
// parse_encryption_info
// ---------------------------------------------------------------------------

/// Parse the EncryptionInfo from page 0. Returns `None` if no Agile
/// Encryption is present, `Some(AgileParams)` if successfully parsed.
pub(crate) fn parse_encryption_info(page0: &[u8]) -> Result<Option<AgileParams>, FileError> {
    let offset = db_header::ENCRYPTION_INFO_OFFSET;
    if page0.len() < offset + 2 {
        return Ok(None);
    }

    // EncryptionInfo length is stored as u16 LE. This is sufficient because
    // the info must fit within page 0 (4096 bytes).
    let info_len = u16::from_le_bytes([page0[offset], page0[offset + 1]]) as usize;
    if info_len == 0 {
        return Ok(None);
    }

    let info_start = offset + 2;
    let info_end = info_start + info_len;
    if page0.len() < info_end {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionInfo extends beyond page 0".to_string(),
        });
    }

    let info_data = &page0[info_start..info_end];

    // First 4 bytes: version (u16 LE) + reserved (u16 LE)
    if info_data.len() < 4 {
        return Ok(None);
    }
    let version = u16::from_le_bytes([info_data[0], info_data[1]]);
    let reserved = u16::from_le_bytes([info_data[2], info_data[3]]);

    if version != 4 || reserved != 4 {
        return Err(FileError::UnsupportedEncryption {
            reason: format!(
                "unsupported EncryptionInfo version={version}, reserved={reserved} (expected 4,4 for Agile)"
            ),
        });
    }

    // The rest is XML — parse with quick-xml
    let xml_bytes = &info_data[4..];

    let mut reader = quick_xml::Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut key_data: Option<KeyDataAttrs> = None;
    let mut enc_key: Option<EncryptedKeyAttrs> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"keyData" => {
                        key_data = Some(parse_key_data(e)?);
                    }
                    b"encryptedKey" => {
                        enc_key = Some(parse_encrypted_key(e)?);
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(err) => {
                return Err(FileError::UnsupportedEncryption {
                    reason: format!("EncryptionInfo XML parse error: {err}"),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    let kd = key_data.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: "missing <keyData> in EncryptionInfo XML".to_string(),
    })?;
    let ek = enc_key.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: "missing <encryptedKey> in EncryptionInfo XML".to_string(),
    })?;

    Ok(Some(AgileParams {
        key_bits: kd.key_bits,
        block_size: kd.block_size,
        hash_algorithm: kd.hash_algorithm,
        salt_value: kd.salt_value,
        pe_spin_count: ek.spin_count,
        pe_salt_value: ek.salt_value,
        pe_hash_algorithm: ek.hash_algorithm,
        pe_key_bits: ek.key_bits,
        pe_block_size: ek.block_size,
        encrypted_verifier_hash_input: ek.encrypted_verifier_hash_input,
        encrypted_verifier_hash_value: ek.encrypted_verifier_hash_value,
        encrypted_key_value: ek.encrypted_key_value,
    }))
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive an encryption key using the Agile Encryption key derivation scheme.
///
/// 1. H0 = hash(salt + password_utf16le)
/// 2. Hn = hash(n_u32le + H_{n-1}) for spinCount iterations
/// 3. Hfinal = hash(H_last + blockKey)
/// 4. Pad/truncate to cbRequiredKeyLength bytes
fn derive_key(
    password: &str,
    salt: &[u8],
    spin_count: u32,
    block_key: &[u8],
    hash_algo: HashAlgorithm,
    key_bits: usize,
) -> Zeroizing<Vec<u8>> {
    // H0 = hash(salt + password_utf16le)
    let password_utf16: Vec<u8> = password
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();

    let mut buf = Vec::with_capacity(salt.len() + password_utf16.len());
    buf.extend_from_slice(salt);
    buf.extend_from_slice(&password_utf16);
    let mut h = hash_bytes(hash_algo, &buf);

    // Hn = hash(n_u32le + H_{n-1})
    for n in 0..spin_count {
        let mut iter_buf = Vec::with_capacity(4 + h.len());
        iter_buf.extend_from_slice(&n.to_le_bytes());
        iter_buf.extend_from_slice(&h);
        h = hash_bytes(hash_algo, &iter_buf);
    }

    // Hfinal = hash(H_last + blockKey)
    let mut final_buf = Vec::with_capacity(h.len() + block_key.len());
    final_buf.extend_from_slice(&h);
    final_buf.extend_from_slice(block_key);
    let h_final = hash_bytes(hash_algo, &final_buf);

    // Pad or truncate to key_bits / 8
    let key_len = key_bits / 8;
    if h_final.len() >= key_len {
        Zeroizing::new(h_final[..key_len].to_vec())
    } else {
        let mut padded = h_final;
        padded.resize(key_len, HASH_PAD_BYTE);
        Zeroizing::new(padded)
    }
}

// ---------------------------------------------------------------------------
// AES-CBC decryption helper
// ---------------------------------------------------------------------------

/// Decrypt data using AES-CBC with the given key and IV.
/// Returns the decrypted data (no PKCS7 unpadding — the caller handles truncation).
fn aes_cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, FileError> {
    // AES-CBC requires data to be a multiple of block size (16).
    // Pad input to block boundary if needed.
    let block_size = 16;
    let padded_len = if data.len() % block_size != 0 {
        (data.len() / block_size + 1) * block_size
    } else {
        data.len()
    };
    let mut buf = vec![0u8; padded_len];
    buf[..data.len()].copy_from_slice(data);

    // Ensure IV is exactly 16 bytes
    let mut iv16 = [0u8; 16];
    let copy_len = iv.len().min(16);
    iv16[..copy_len].copy_from_slice(&iv[..copy_len]);

    match key.len() {
        16 => {
            let mut key16 = [0u8; 16];
            key16.copy_from_slice(key);
            Aes128CbcDec::new(&key16.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-CBC decryption failed".to_string(),
                })?;
        }
        32 => {
            let mut key32 = [0u8; 32];
            key32.copy_from_slice(key);
            Aes256CbcDec::new(&key32.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-CBC decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported AES key size: {other} bytes"),
            });
        }
    }

    Ok(buf)
}

// ---------------------------------------------------------------------------
// verify_password
// ---------------------------------------------------------------------------

/// Verify the password against the EncryptionInfo parameters.
/// On success, returns the decrypted database key (encryptedKeyValue).
pub(crate) fn verify_password(
    params: &AgileParams,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, FileError> {
    let algo = params.pe_hash_algorithm;
    let salt = &params.pe_salt_value;
    let spin = params.pe_spin_count;
    let key_bits = params.pe_key_bits;

    // Derive the three keys
    let key_input = derive_key(
        password,
        salt,
        spin,
        &VERIFIER_HASH_INPUT_BLOCK_KEY,
        algo,
        key_bits,
    );
    let key_hash_value = derive_key(
        password,
        salt,
        spin,
        &VERIFIER_HASH_VALUE_BLOCK_KEY,
        algo,
        key_bits,
    );
    let key_enc_key = derive_key(
        password,
        salt,
        spin,
        &ENCRYPTED_KEY_VALUE_BLOCK_KEY,
        algo,
        key_bits,
    );

    // IV for password encryptor decryption: salt padded/truncated to pe_block_size
    let iv = make_iv(salt, params.pe_block_size);

    // Decrypt verifierHashInput
    let verifier = aes_cbc_decrypt(&key_input, &iv, &params.encrypted_verifier_hash_input)?;

    // Decrypt verifierHashValue
    let expected_hash_full =
        aes_cbc_decrypt(&key_hash_value, &iv, &params.encrypted_verifier_hash_value)?;

    // Verify: hash(verifier) == expected_hash (truncated to hash size)
    let hash_size = algo.hash_size();
    let actual_hash = hash_bytes(algo, &verifier);

    let expected_hash = if expected_hash_full.len() >= hash_size {
        &expected_hash_full[..hash_size]
    } else {
        &expected_hash_full
    };

    if actual_hash[..hash_size].ct_eq(expected_hash).unwrap_u8() != 1 {
        return Err(FileError::InvalidPassword);
    }

    // Decrypt the database key
    let db_key = aes_cbc_decrypt(&key_enc_key, &iv, &params.encrypted_key_value)?;

    // Truncate to keyData keyBits / 8
    let db_key_len = params.key_bits / 8;
    Ok(Zeroizing::new(db_key[..db_key_len].to_vec()))
}

// ---------------------------------------------------------------------------
// Page decryption
// ---------------------------------------------------------------------------

/// Build an IV by padding/truncating data to the target block size.
fn make_iv(data: &[u8], block_size: usize) -> Vec<u8> {
    if data.len() >= block_size {
        data[..block_size].to_vec()
    } else {
        let mut iv = vec![HASH_PAD_BYTE; block_size];
        iv[..data.len()].copy_from_slice(data);
        iv
    }
}

/// Decrypt a data page using Agile Encryption (Access-specific scheme).
///
/// IV = hash(db_salt + block_key_bytes), padded/truncated to blockSize.
/// block_key = page_number XOR db_encoding_key.
pub(crate) fn decrypt_page_agile(
    buf: &mut [u8],
    params: &AgileParams,
    db_key: &[u8],
    db_encoding_key: u32,
    page: u32,
) -> Result<(), FileError> {
    if buf.is_empty() {
        return Ok(());
    }

    let block_key = page ^ db_encoding_key;
    let block_key_bytes = block_key.to_le_bytes();

    // IV = hash(keyData.salt + block_key_bytes)
    let mut iv_input = Vec::with_capacity(params.salt_value.len() + 4);
    iv_input.extend_from_slice(&params.salt_value);
    iv_input.extend_from_slice(&block_key_bytes);
    let iv_hash = hash_bytes(params.hash_algorithm, &iv_input);
    let iv = make_iv(&iv_hash, params.block_size);

    // Decrypt entire page with AES-CBC
    // Ensure buffer is block-aligned for AES
    let aes_block = 16;
    let decrypt_len = (buf.len() / aes_block) * aes_block;
    if decrypt_len == 0 {
        return Ok(());
    }

    let mut iv16 = [0u8; 16];
    let copy_len = iv.len().min(16);
    iv16[..copy_len].copy_from_slice(&iv[..copy_len]);

    match db_key.len() {
        16 => {
            let mut key16 = [0u8; 16];
            key16.copy_from_slice(db_key);
            Aes128CbcDec::new(&key16.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-CBC page decryption failed".to_string(),
                })?;
        }
        32 => {
            let mut key32 = [0u8; 32];
            key32.copy_from_slice(db_key);
            Aes256CbcDec::new(&key32.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-CBC page decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported db_key length: {other} bytes"),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{rc4_transform, HEADER_RC4_KEY};
    use crate::format::JetVersion;

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

    fn hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hash_algorithm_from_str_valid() {
        assert_eq!(HashAlgorithm::from_str("SHA1").unwrap(), HashAlgorithm::Sha1);
        assert_eq!(HashAlgorithm::from_str("SHA256").unwrap(), HashAlgorithm::Sha256);
        assert_eq!(HashAlgorithm::from_str("SHA384").unwrap(), HashAlgorithm::Sha384);
        assert_eq!(HashAlgorithm::from_str("SHA512").unwrap(), HashAlgorithm::Sha512);
    }

    #[test]
    fn hash_algorithm_from_str_invalid() {
        assert!(HashAlgorithm::from_str("MD5").is_err());
    }

    #[test]
    fn hash_algorithm_hash_size() {
        assert_eq!(HashAlgorithm::Sha1.hash_size(), 20);
        assert_eq!(HashAlgorithm::Sha256.hash_size(), 32);
        assert_eq!(HashAlgorithm::Sha384.hash_size(), 48);
        assert_eq!(HashAlgorithm::Sha512.hash_size(), 64);
    }

    #[test]
    fn hash_bytes_sha256() {
        let data = b"hello";
        let result = hash_bytes(HashAlgorithm::Sha256, data);
        assert_eq!(result.len(), 32);
        // Known SHA-256 of "hello"
        assert_eq!(
            hex(&result),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn hash_bytes_sha1() {
        let data = b"hello";
        let result = hash_bytes(HashAlgorithm::Sha1, data);
        assert_eq!(result.len(), 20);
        assert_eq!(
            hex(&result),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
    }

    #[test]
    fn derive_key_length() {
        let key = derive_key("test", &[0u8; 16], 100, &[0u8; 8], HashAlgorithm::Sha256, 128);
        assert_eq!(key.len(), 16); // 128 bits / 8
    }

    #[test]
    fn derive_key_length_256() {
        let key = derive_key("test", &[0u8; 16], 100, &[0u8; 8], HashAlgorithm::Sha256, 256);
        assert_eq!(key.len(), 32); // 256 bits / 8
    }

    #[test]
    fn derive_key_deterministic() {
        // Same inputs must produce the same output
        let k1 = derive_key("pw", &[1, 2, 3, 4], 10, &[0xAA; 8], HashAlgorithm::Sha256, 128);
        let k2 = derive_key("pw", &[1, 2, 3, 4], 10, &[0xAA; 8], HashAlgorithm::Sha256, 128);
        assert_eq!(*k1, *k2);

        // Different block key → different output
        let k3 = derive_key("pw", &[1, 2, 3, 4], 10, &[0xBB; 8], HashAlgorithm::Sha256, 128);
        assert_ne!(*k1, *k3);
    }

    #[test]
    fn make_iv_exact() {
        let data = vec![1u8; 16];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
        assert_eq!(iv, data);
    }

    #[test]
    fn make_iv_truncate() {
        let data = vec![1u8; 32];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
    }

    #[test]
    fn make_iv_pad() {
        let data = vec![1u8; 8];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
        assert_eq!(&iv[..8], &[1u8; 8]);
        assert_eq!(&iv[8..], &[HASH_PAD_BYTE; 8]);
    }

    #[test]
    fn aes_cbc_decrypt_roundtrip() {
        // Encrypt with known key/iv, then decrypt and verify
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

        let key = [0x42u8; 16];
        let iv = [0u8; 16];
        let plaintext = [0xAAu8; 32]; // 2 AES blocks

        let mut buf = plaintext.to_vec();
        Aes128CbcEnc::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_cbc_decrypt(&key, &iv, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn parse_encryption_info_no_data() {
        let page0 = vec![0u8; 4096];
        let result = parse_encryption_info(&page0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_encryption_info_too_small() {
        let page0 = vec![0u8; 0x299];
        let result = parse_encryption_info(&page0).unwrap();
        assert!(result.is_none());
    }

    /// Test with real .accdb file if available
    #[test]
    fn parse_and_verify_enc_v2007() {
        let path = skip_if_missing!("enc_vbaV2007.accdb");

        // Read page 0 manually
        use std::io::Read;
        let mut file = std::fs::File::open(&path).unwrap();
        let mut page0 = vec![0u8; 4096];
        file.read_exact(&mut page0).unwrap();

        // Decrypt header region (RC4)
        let version = JetVersion::from_byte(page0[db_header::VERSION]).unwrap();
        assert!(version.is_accdb());

        // RC4 decrypt header
        let enc_len = 128;
        let end = db_header::ENCRYPTED_START + enc_len;
        rc4_transform(&HEADER_RC4_KEY, &mut page0[db_header::ENCRYPTED_START..end]);

        // Parse EncryptionInfo
        let params = parse_encryption_info(&page0).unwrap();
        assert!(params.is_some(), "EncryptionInfo should be present");
        let params = params.unwrap();

        // Verify correct password
        let db_key = verify_password(&params, "1234567890");
        assert!(db_key.is_ok(), "correct password should verify: {:?}", db_key.err());
        let db_key = db_key.unwrap();
        assert_eq!(db_key.len(), params.key_bits / 8);

        // Verify incorrect password
        let result = verify_password(&params, "wrongpassword");
        assert!(matches!(result, Err(FileError::InvalidPassword)));
    }

}
