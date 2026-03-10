# Technical Debt and Future Improvements

## Checksum System Enhancement

**Current State**: The Rust implementation uses a simpler direct CRC32C calculation approach.

**Haskell Reference**: The Haskell `lsm-tree` implementation has comprehensive checksum support including:
- Checksum files for storing multiple file checksums
- Cross-platform file I/O abstractions (`HasFS`, `HasBlockIO`)
- Explicit cache management (`dropCache`)
- Integration with block I/O for direct access
- Full validation pipeline with rich error types (`FileCorruptedError`, etc.)

**Recommendation**: When time permits, evaluate if adopting more of the Haskell checksum infrastructure would improve:
- Error detection and recovery
- Cross-platform compatibility
- Performance (especially with direct I/O)
- Debugging capabilities

See: `lsm-tree/src-core/Database/LSMTree/Internal/CRC32C.hs` (700+ lines)
See: `lsm-tree/src-core/Database/LSMTree/Internal/ChecksumHandle.hs` (89 lines)

**Priority**: Low - Current implementation passes all 10,000+ conformance tests

---

## Future Feature Ideas

- Background compaction threads
- More sophisticated merge scheduling
- io_uring optimizations for batched I/O
- Compression support for SSTables
- More detailed compaction metrics and monitoring
