# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2026-03-09

### Fixed
- Removed incorrect WAL (Write-Ahead Log) documentation from README
  - This crate does not implement WAL; writes are lost on crash until a snapshot is saved
  - The README incorrectly listed WAL recovery features that don't exist

## [1.0.0] - 2026-03-09

### Added
- Complete LSM tree implementation in pure Rust
- Reference-counted snapshots for fast rollback (critical for blockchain reorgs)
- Tiered, leveled, and hybrid compaction strategies
- Bloom filters for fast negative lookups
- Range query support with proper tombstone handling
- Comprehensive conformance testing against Haskell reference implementation
  - 10,000+ property-based tests with 100% pass rate
  - Auto-generated test harness from Haskell lsm-tree
- Batch operations for efficient bulk insertions
- Snapshot save/restore functionality
- Cross-format validation between Rust and Haskell implementations
- Apache 2.0 license
- Comprehensive documentation (README, TESTING, BENCHMARKS)

### Changed
- Optimized compaction to use `sort_by_key` for better performance
- Updated base64 API to use modern Engine trait
- Changed `&PathBuf` to `&Path` in function signatures for better ergonomics
- `CRC32C::to_hex` now takes self by value (Copy type optimization)

### Fixed
- Range query tombstone handling for correct iteration semantics
- Rollback operations now properly handle post-snapshot insertions
- All clippy warnings resolved
- Deprecated base64 API calls updated

### Technical Details
- Pure Rust implementation (no FFI or Haskell runtime dependencies)
- Optimized for UTxO workloads in blockchain indexing
- Fast snapshots (< 10ms) via reference counting
- Fast rollback (< 1s) for chain reorganizations
- Leveled compaction with LazyLevelling policy
- Session locking prevents concurrent database access

## [0.1.0] - Initial Development
- Internal development version
- Core LSM tree functionality
- Initial test suite
