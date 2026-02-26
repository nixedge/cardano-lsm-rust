// Port of Database.LSMTree.Internal.CRC32C from Haskell
//
// Functionality related to CRC-32C (Castagnoli) checksums:
// * Support for calculating checksums while incrementally writing files
// * Support for verifying checksums of files
// * Support for a text file format listing file checksums
//
// The checksum file format matches Haskell's BSD-style format:
//
// ```
// CRC32C (keyops) = fd040004
// CRC32C (blobs) = 5a3b820c
// CRC32C (filter) = 6653e178
// CRC32C (index) = f4ec6724
// ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

/// CRC-32C (Castagnoli) checksum value
///
/// This is a wrapper around u32 to provide type safety for checksum values.
/// Uses the CRC-32C polynomial (0x1EDC6F41) which has hardware acceleration
/// on modern Intel/AMD processors (via SSE4.2 instructions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CRC32C(pub u32);

impl CRC32C {
    /// Initial CRC32C value (equivalent to crc32c of empty bytestring)
    pub const INITIAL: CRC32C = CRC32C(0);

    /// Compute CRC32C of a byte slice in one shot
    pub fn hash(data: &[u8]) -> Self {
        CRC32C(crc32fast::hash(data))
    }

    /// Read entire file and compute its CRC32C checksum
    ///
    /// Reads the file in chunks to avoid loading large files into memory.
    pub fn read_file_crc32c<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut hasher = crc32fast::Hasher::new();
        let mut buffer = vec![0u8; 8192]; // 8KB chunks

        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        Ok(CRC32C(hasher.finalize()))
    }

    /// Format as hex string (8 hex digits, lowercase)
    pub fn to_hex(&self) -> String {
        format!("{:08x}", self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, String> {
        u32::from_str_radix(s, 16)
            .map(CRC32C)
            .map_err(|e| format!("Failed to parse hex: {}", e))
    }
}

/// Map of filenames to their CRC32C checksums
///
/// This represents the contents of a .checksum file.
/// File names must not include characters '(', ')', or '\n'.
pub type ChecksumsFile = HashMap<String, CRC32C>;

/// Parse a .checksum file in BSD-style format
///
/// Format:
/// ```text
/// CRC32C (filename) = hexvalue
/// ```
///
/// Example:
/// ```text
/// CRC32C (keyops) = fd040004
/// CRC32C (blobs) = 5a3b820c
/// CRC32C (filter) = 6653e178
/// CRC32C (index) = f4ec6724
/// ```
pub fn parse_checksums_file(content: &str) -> Result<ChecksumsFile, String> {
    let mut checksums = HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Parse line: "CRC32C (filename) = hexvalue"
        let (filename, hex_value) = parse_checksum_line(line)
            .map_err(|e| format!("Line {}: {}", line_num + 1, e))?;

        let crc = CRC32C::from_hex(&hex_value)
            .map_err(|e| format!("Line {}: {}", line_num + 1, e))?;

        checksums.insert(filename, crc);
    }

    Ok(checksums)
}

/// Parse a single line: "CRC32C (filename) = hexvalue"
fn parse_checksum_line(line: &str) -> Result<(String, String), String> {
    // Expected format: "CRC32C (filename) = hexvalue"

    // Check prefix
    if !line.starts_with("CRC32C (") {
        return Err(format!("Expected line to start with 'CRC32C (', got: {}", line));
    }

    // Find the closing parenthesis
    let close_paren = line.find(')').ok_or_else(|| {
        format!("Expected ')' in line: {}", line)
    })?;

    // Extract filename (between "CRC32C (" and ")")
    let filename = &line[8..close_paren]; // 8 = length of "CRC32C ("

    // Validate filename (no newlines, parentheses)
    if filename.contains('\n') || filename.contains('(') || filename.contains(')') {
        return Err(format!("Invalid characters in filename: {}", filename));
    }

    // Rest should be " = hexvalue"
    let rest = &line[close_paren + 1..].trim();
    if !rest.starts_with('=') {
        return Err(format!("Expected '=' after filename, got: {}", rest));
    }

    let hex_value = rest[1..].trim();

    // Validate hex (should be exactly 8 hex digits)
    if hex_value.len() != 8 {
        return Err(format!("Expected 8 hex digits, got: {}", hex_value));
    }

    Ok((filename.to_string(), hex_value.to_string()))
}

/// Format checksums as BSD-style checksum file
///
/// Output format:
/// ```text
/// CRC32C (filename) = hexvalue
/// ```
///
/// Files are sorted alphabetically for deterministic output.
pub fn format_checksums_file(checksums: &ChecksumsFile) -> String {
    let mut lines: Vec<_> = checksums.iter().collect();
    lines.sort_by_key(|(name, _)| *name);

    let mut output = String::new();
    for (filename, crc) in lines {
        output.push_str(&format!("CRC32C ({}) = {}\n", filename, crc.to_hex()));
    }
    output
}

/// Read a .checksum file from disk
pub fn read_checksums_file<P: AsRef<Path>>(path: P) -> io::Result<ChecksumsFile> {
    let content = std::fs::read_to_string(path)?;
    parse_checksums_file(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write a .checksum file to disk
///
/// Uses atomic write pattern: write to temp file, then rename.
/// This ensures the checksum file is never partially written.
pub fn write_checksums_file<P: AsRef<Path>>(path: P, checksums: &ChecksumsFile) -> io::Result<()> {
    let path = path.as_ref();
    let content = format_checksums_file(checksums);

    // Write to temp file first
    let temp_path = path.with_extension("checksum.tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?; // Ensure data is on disk
    drop(file);

    // Atomic rename
    std::fs::rename(&temp_path, path)?;

    // Sync parent directory to ensure rename is durable
    if let Some(parent) = path.parent() {
        let dir = File::open(parent)?;
        dir.sync_all()?;
    }

    Ok(())
}

/// Get a specific checksum from a ChecksumsFile
pub fn get_checksum(
    checksums_file: &ChecksumsFile,
    filename: &str,
) -> Result<CRC32C, String> {
    checksums_file
        .get(filename)
        .copied()
        .ok_or_else(|| format!("Checksum not found for file: {}", filename))
}

/// Verify that a file's checksum matches the expected value
///
/// Returns Ok(()) if the checksum matches, otherwise returns an error.
pub fn check_crc<P: AsRef<Path>>(
    path: P,
    expected: CRC32C,
) -> io::Result<()> {
    let path = path.as_ref();
    let computed = CRC32C::read_file_crc32c(path)?;

    if computed != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Checksum mismatch for {}: expected {}, got {}",
                path.display(),
                expected.to_hex(),
                computed.to_hex()
            ),
        ));
    }

    Ok(())
}

/// Expect a specific checksum for a file, error if it doesn't match
///
/// This is a convenience wrapper around check_crc that provides
/// more detailed error messages.
pub fn expect_checksum<P: AsRef<Path>>(
    path: P,
    checksums_file: &ChecksumsFile,
    filename: &str,
) -> io::Result<()> {
    let path = path.as_ref();
    let expected = get_checksum(checksums_file, filename)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    check_crc(path, expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_crc() {
        let crc = CRC32C::INITIAL;
        assert_eq!(crc.0, 0);
    }

    #[test]
    fn test_hash() {
        let crc = CRC32C::hash(b"hello world");
        assert_ne!(crc.0, 0);
    }

    #[test]
    fn test_hash_empty() {
        let crc = CRC32C::hash(b"");
        assert_eq!(crc, CRC32C::INITIAL);
    }

    #[test]
    fn test_hex_format() {
        let crc = CRC32C(0xfd040004);
        assert_eq!(crc.to_hex(), "fd040004");
    }

    #[test]
    fn test_hex_parse() {
        let crc = CRC32C::from_hex("fd040004").unwrap();
        assert_eq!(crc.0, 0xfd040004);
    }

    #[test]
    fn test_parse_checksum_line() {
        let line = "CRC32C (keyops) = fd040004";
        let (filename, hex) = parse_checksum_line(line).unwrap();
        assert_eq!(filename, "keyops");
        assert_eq!(hex, "fd040004");
    }

    #[test]
    fn test_parse_checksums_file() {
        let content = r#"
CRC32C (keyops) = fd040004
CRC32C (blobs) = 5a3b820c
CRC32C (filter) = 6653e178
CRC32C (index) = f4ec6724
"#;
        let checksums = parse_checksums_file(content).unwrap();
        assert_eq!(checksums.len(), 4);
        assert_eq!(checksums.get("keyops").unwrap().0, 0xfd040004);
        assert_eq!(checksums.get("blobs").unwrap().0, 0x5a3b820c);
    }

    #[test]
    fn test_format_checksums_file() {
        let mut checksums = HashMap::new();
        checksums.insert("keyops".to_string(), CRC32C(0xfd040004));
        checksums.insert("blobs".to_string(), CRC32C(0x5a3b820c));

        let output = format_checksums_file(&checksums);

        // Should be sorted alphabetically
        assert!(output.contains("CRC32C (blobs) = 5a3b820c\n"));
        assert!(output.contains("CRC32C (keyops) = fd040004\n"));

        // blobs should come before keyops (alphabetical)
        let blobs_pos = output.find("blobs").unwrap();
        let keyops_pos = output.find("keyops").unwrap();
        assert!(blobs_pos < keyops_pos);
    }

    #[test]
    fn test_invalid_checksum_line() {
        // Missing closing paren
        assert!(parse_checksum_line("CRC32C (keyops = fd040004").is_err());

        // Missing equals sign
        assert!(parse_checksum_line("CRC32C (keyops) fd040004").is_err());

        // Wrong hex length
        assert!(parse_checksum_line("CRC32C (keyops) = fd04").is_err());
    }
}
