//! Lightweight, dependency-minimal ZIP archive reader.
//!
//! EPUB files are ordinary ZIP archives and we only ever need to pull a handful
//! of named XML/XHTML entries out of them. Rather than depend on the full-featured
//! `zip` crate, this module parses just enough of the ZIP format to do that:
//!
//! 1. Locate the End Of Central Directory (EOCD) record at the tail of the file.
//! 2. Walk the Central Directory to build a catalog of entries (name, compression
//!    method, sizes, and the offset of each Local File Header).
//! 3. On demand, parse a Local File Header and inflate (or copy) the entry bytes.
//!
//! Only the two compression methods EPUB uses are supported: `Stored` (0) and
//! `Deflate` (8). DEFLATE inflation is delegated to `miniz_oxide`, a pure-Rust
//! implementation with no C dependencies, which keeps cross-compilation to the
//! iOS/Android targets clean.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_DIR_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;

/// Fixed size (without the variable-length comment) of an EOCD record.
const EOCD_MIN_SIZE: usize = 22;
/// Fixed size (without variable-length name/extra/comment) of a central directory entry.
const CENTRAL_DIR_FIXED_SIZE: usize = 46;
/// Fixed size (without variable-length name/extra) of a local file header.
const LOCAL_HEADER_FIXED_SIZE: usize = 30;

/// A ZIP64 sentinel; if any 32-bit size/offset field holds this value the archive
/// uses the ZIP64 extensions, which we deliberately do not support (EPUBs never need them).
const ZIP64_SENTINEL_U32: u32 = 0xFFFF_FFFF;

const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

#[derive(Debug)]
pub enum ZipReadError {
    /// The bytes are not a recognizable (non-ZIP64) ZIP archive.
    InvalidArchive(String),
    /// No entry with the requested name exists in the central directory.
    EntryNotFound(String),
    /// The entry uses a compression method other than Stored or Deflate.
    UnsupportedCompression(u16),
    /// DEFLATE inflation failed.
    Decompress(String),
    /// Reading the backing file failed.
    Io(std::io::Error),
}

impl std::fmt::Display for ZipReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipReadError::InvalidArchive(m) => write!(f, "invalid zip archive: {m}"),
            ZipReadError::EntryNotFound(n) => write!(f, "entry not found in archive: {n}"),
            ZipReadError::UnsupportedCompression(m) => {
                write!(f, "unsupported zip compression method: {m}")
            }
            ZipReadError::Decompress(m) => write!(f, "zip decompression failed: {m}"),
            ZipReadError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ZipReadError {}

impl From<std::io::Error> for ZipReadError {
    fn from(err: std::io::Error) -> Self {
        ZipReadError::Io(err)
    }
}

/// A single catalog entry parsed from the central directory.
#[derive(Debug, Clone)]
struct CentralDirEntry {
    compression_method: u16,
    compressed_size: u64,
    local_header_offset: u64,
}

/// An in-memory ZIP archive. The whole file is held in memory; EPUBs are small
/// enough that this is simpler and more robust than seeking around the file.
pub struct ZipArchive {
    data: Vec<u8>,
    entries: HashMap<String, CentralDirEntry>,
}

impl ZipArchive {
    /// Open and index a ZIP archive from a filesystem path.
    pub fn open(path: &str) -> Result<Self, ZipReadError> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Self::from_bytes(data)
    }

    /// Index a ZIP archive already resident in memory.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, ZipReadError> {
        let entries = parse_central_directory(&data)?;
        Ok(Self { data, entries })
    }

    /// Read and decompress a single entry by its full archive path.
    pub fn by_name(&self, name: &str) -> Result<Vec<u8>, ZipReadError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| ZipReadError::EntryNotFound(name.to_string()))?;

        // The central directory records the local header offset, but the name and
        // extra-field lengths in the local header can differ from the central
        // directory's, so we must re-read them from the local header itself.
        let lho = entry.local_header_offset as usize;
        if lho + LOCAL_HEADER_FIXED_SIZE > self.data.len() {
            return Err(ZipReadError::InvalidArchive(
                "local header offset out of bounds".into(),
            ));
        }
        if read_u32(&self.data, lho) != LOCAL_HEADER_SIGNATURE {
            return Err(ZipReadError::InvalidArchive(
                "bad local file header signature".into(),
            ));
        }
        let name_len = read_u16(&self.data, lho + 26) as usize;
        let extra_len = read_u16(&self.data, lho + 28) as usize;
        let data_start = lho + LOCAL_HEADER_FIXED_SIZE + name_len + extra_len;
        let data_end = data_start + entry.compressed_size as usize;
        if data_end > self.data.len() {
            return Err(ZipReadError::InvalidArchive(
                "entry data extends past end of archive".into(),
            ));
        }
        let compressed = &self.data[data_start..data_end];

        match entry.compression_method {
            METHOD_STORED => Ok(compressed.to_vec()),
            METHOD_DEFLATE => {
                miniz_oxide::inflate::decompress_to_vec(compressed).map_err(|e| {
                    ZipReadError::Decompress(format!(
                        "{:?} after {} bytes",
                        e.status,
                        e.output.len()
                    ))
                })
            }
            other => Err(ZipReadError::UnsupportedCompression(other)),
        }
    }
}

/// Scan backward from the end of the file for the EOCD signature and return its offset.
fn find_eocd(data: &[u8]) -> Result<usize, ZipReadError> {
    if data.len() < EOCD_MIN_SIZE {
        return Err(ZipReadError::InvalidArchive("file smaller than EOCD".into()));
    }
    // The EOCD comment is at most 65535 bytes, so we only need to look back that
    // far plus the fixed EOCD size.
    let max_back = (EOCD_MIN_SIZE + u16::MAX as usize).min(data.len());
    let scan_start = data.len() - max_back;
    // Walk backward over every position the 4-byte signature could occupy.
    for offset in (scan_start..=data.len() - 4).rev() {
        if read_u32(data, offset) == EOCD_SIGNATURE {
            return Ok(offset);
        }
    }
    Err(ZipReadError::InvalidArchive(
        "end of central directory record not found".into(),
    ))
}

/// Parse the EOCD and central directory into a name->entry catalog.
fn parse_central_directory(
    data: &[u8],
) -> Result<HashMap<String, CentralDirEntry>, ZipReadError> {
    let eocd = find_eocd(data)?;

    let total_entries = read_u16(data, eocd + 10) as usize;
    let cd_size = read_u32(data, eocd + 12);
    let cd_offset = read_u32(data, eocd + 16);

    if cd_offset == ZIP64_SENTINEL_U32 || cd_size == ZIP64_SENTINEL_U32 {
        return Err(ZipReadError::InvalidArchive(
            "ZIP64 archives are not supported".into(),
        ));
    }

    let mut cursor = cd_offset as usize;
    let cd_end = cursor + cd_size as usize;
    if cd_end > data.len() {
        return Err(ZipReadError::InvalidArchive(
            "central directory extends past end of archive".into(),
        ));
    }

    let mut entries = HashMap::with_capacity(total_entries);
    for _ in 0..total_entries {
        if cursor + CENTRAL_DIR_FIXED_SIZE > data.len() {
            return Err(ZipReadError::InvalidArchive(
                "central directory entry truncated".into(),
            ));
        }
        if read_u32(data, cursor) != CENTRAL_DIR_SIGNATURE {
            return Err(ZipReadError::InvalidArchive(
                "bad central directory signature".into(),
            ));
        }

        let compression_method = read_u16(data, cursor + 10);
        let compressed_size = read_u32(data, cursor + 20);
        let name_len = read_u16(data, cursor + 28) as usize;
        let extra_len = read_u16(data, cursor + 30) as usize;
        let comment_len = read_u16(data, cursor + 32) as usize;
        let local_header_offset = read_u32(data, cursor + 42);

        if compressed_size == ZIP64_SENTINEL_U32
            || local_header_offset == ZIP64_SENTINEL_U32
        {
            return Err(ZipReadError::InvalidArchive(
                "ZIP64 entry fields are not supported".into(),
            ));
        }

        let name_start = cursor + CENTRAL_DIR_FIXED_SIZE;
        let name_end = name_start + name_len;
        if name_end > data.len() {
            return Err(ZipReadError::InvalidArchive(
                "central directory file name truncated".into(),
            ));
        }
        let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();

        // Directory entries (trailing '/') carry no useful payload; skip them.
        if !name.ends_with('/') {
            entries.insert(
                name,
                CentralDirEntry {
                    compression_method,
                    compressed_size: compressed_size as u64,
                    local_header_offset: local_header_offset as u64,
                },
            );
        }

        cursor = name_end + extra_len + comment_len;
    }

    Ok(entries)
}

/// Read a little-endian u16 at `offset`. Callers must ensure `offset + 2 <= data.len()`.
#[inline]
fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Read a little-endian u32 at `offset`. Callers must ensure `offset + 4 <= data.len()`.
#[inline]
fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A description of one entry to place into a test archive.
    struct TestEntry {
        name: &'static str,
        method: u16,
        /// Uncompressed payload.
        data: Vec<u8>,
    }

    /// Build a minimal but spec-valid ZIP archive in memory. CRCs are left zero
    /// because our reader does not verify them.
    fn build_zip(entries: &[TestEntry]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut records: Vec<(u32, u32, u32)> = Vec::new(); // (local_offset, comp_size, uncomp_size)

        for e in entries {
            let local_offset = out.len() as u32;
            let stored: Vec<u8> = match e.method {
                METHOD_STORED => e.data.clone(),
                METHOD_DEFLATE => deflate_raw(&e.data),
                _ => panic!("unsupported method in test builder"),
            };
            let comp_size = stored.len() as u32;
            let uncomp_size = e.data.len() as u32;

            // Local file header.
            out.extend_from_slice(&LOCAL_HEADER_SIGNATURE.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&e.method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc32
            out.extend_from_slice(&comp_size.to_le_bytes());
            out.extend_from_slice(&uncomp_size.to_le_bytes());
            out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(e.name.as_bytes());
            out.extend_from_slice(&stored);

            records.push((local_offset, comp_size, uncomp_size));
        }

        let cd_offset = out.len() as u32;
        for (e, &(local_offset, comp_size, uncomp_size)) in entries.iter().zip(&records) {
            out.extend_from_slice(&CENTRAL_DIR_SIGNATURE.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version made by
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&e.method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc32
            out.extend_from_slice(&comp_size.to_le_bytes());
            out.extend_from_slice(&uncomp_size.to_le_bytes());
            out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(&0u16.to_le_bytes()); // comment len
            out.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            out.extend_from_slice(&local_offset.to_le_bytes());
            out.extend_from_slice(e.name.as_bytes());
        }
        let cd_size = out.len() as u32 - cd_offset;

        // End of central directory.
        out.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // disk number
        out.extend_from_slice(&0u16.to_le_bytes()); // disk with cd
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len

        out
    }

    /// Produce a raw DEFLATE stream (no zlib wrapper), matching what ZIP stores.
    fn deflate_raw(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn reads_stored_entry() {
        let payload = b"hello stored world".to_vec();
        let zip = build_zip(&[TestEntry {
            name: "mimetype",
            method: METHOD_STORED,
            data: payload.clone(),
        }]);
        let archive = ZipArchive::from_bytes(zip).unwrap();
        assert_eq!(archive.by_name("mimetype").unwrap(), payload);
    }

    #[test]
    fn reads_deflated_entry() {
        // Use repetitive content so deflate actually shrinks it.
        let payload = "<html><body>".repeat(500).into_bytes();
        let zip = build_zip(&[TestEntry {
            name: "OEBPS/chapter1.xhtml",
            method: METHOD_DEFLATE,
            data: payload.clone(),
        }]);
        let archive = ZipArchive::from_bytes(zip).unwrap();
        assert_eq!(archive.by_name("OEBPS/chapter1.xhtml").unwrap(), payload);
    }

    #[test]
    fn reads_multiple_entries_and_skips_directories() {
        let zip = build_zip(&[
            TestEntry {
                name: "META-INF/container.xml",
                method: METHOD_DEFLATE,
                data: b"<container/>".repeat(100),
            },
            TestEntry {
                name: "mimetype",
                method: METHOD_STORED,
                data: b"application/epub+zip".to_vec(),
            },
        ]);
        let archive = ZipArchive::from_bytes(zip).unwrap();
        assert_eq!(
            archive.by_name("mimetype").unwrap(),
            b"application/epub+zip"
        );
        assert_eq!(
            archive.by_name("META-INF/container.xml").unwrap(),
            b"<container/>".repeat(100)
        );
    }

    #[test]
    fn missing_entry_errors() {
        let zip = build_zip(&[TestEntry {
            name: "mimetype",
            method: METHOD_STORED,
            data: b"x".to_vec(),
        }]);
        let archive = ZipArchive::from_bytes(zip).unwrap();
        assert!(matches!(
            archive.by_name("nope"),
            Err(ZipReadError::EntryNotFound(_))
        ));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(matches!(
            ZipArchive::from_bytes(vec![0u8; 8]),
            Err(ZipReadError::InvalidArchive(_))
        ));
    }
}
