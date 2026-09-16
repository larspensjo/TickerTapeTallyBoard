/// Number of bytes in one mebibyte.
pub const BYTES_PER_MIB: usize = 1024 * 1024;

/// Maximum body size for ordinary API routes.
pub const API_BODY_LIMIT_BYTES: usize = BYTES_PER_MIB;

/// Maximum body size for the four CSV import routes.
pub const IMPORT_BODY_LIMIT_BYTES: usize = 32 * BYTES_PER_MIB;

/// A body size comfortably inside the import limit.
pub const fn under_import_limit_bytes() -> usize {
    IMPORT_BODY_LIMIT_BYTES / 4
}

/// A body size that is over the import limit by one mebibyte.
pub const fn over_import_limit_bytes() -> usize {
    IMPORT_BODY_LIMIT_BYTES + BYTES_PER_MIB
}
