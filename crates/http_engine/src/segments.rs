//! Segment planning: split a file into N byte ranges.

/// One byte range `[start, end]` (inclusive) of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub idx: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}
