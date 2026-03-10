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

# List of things to remove:

  REMOVE - Iteration Cruft ❌

  These appear to be old code from early iterations that got replaced:

  src/compaction.rs:
  - Compactor.strategy - Not used; compaction uses select_level_compaction() directly
  - Compactor.base_path - Not used
  - Compactor::select_compaction() - Old approach; replaced by select_level_compaction()
  - Compactor::select_tiered_compaction() - Old approach
  - Compactor::select_leveled_compaction() - Old approach
  - CompactionJob struct - Replaced by LevelCompactionJob
  - CompactionResult struct - Still used in compact() method, KEEP THIS ONE

  Haskell evidence: Uses configuration-driven compaction with MergeSchedule, not explicit strategy selection. Your current select_level_compaction() matches the Haskell approach
  better.

  src/atomic_file.rs:
  - AtomicFile::as_writer() - Never used
  - AtomicFile::abort() - Never used (commit or drop pattern works)
  - atomic_write() - Never used (direct AtomicFileWriter usage preferred)

  Haskell evidence: RunBuilder handles atomicity implicitly. The pattern you use (explicit commit) is sufficient.

  src/checksum_handle.rs:
  - ChecksumHandle::from_file() - Never used
  - ChecksumHandle::write() - Never used
  - ChecksumHandle::flush() - Never used
  - ChecksumHandle::sync_all() - Never used
  - ChecksumHandle::current_checksum() - Never used
  - Most write_* methods - Never used

  Haskell evidence: Has comprehensive checksum support, but Rust implementation uses simpler direct CRC32C calculation. These wrappers were over-engineered.
