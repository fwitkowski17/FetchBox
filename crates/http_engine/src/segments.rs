//! Segment planning: split a file into N byte ranges.

pub use dlm_core::SegmentState as Segment;

/// Smallest segment worth a separate connection.
const MIN_SEGMENT: u64 = 256 * 1024;

/// Split `total` bytes into at most `connections` contiguous inclusive ranges.
///
/// Small files get fewer segments (never below [`MIN_SEGMENT`] each) and an empty
/// file gets none. The ranges cover `0..total` exactly, without gaps or overlap.
pub fn plan(total: u64, connections: u32) -> Vec<Segment> {
    if total == 0 {
        return Vec::new();
    }
    let by_size = total.div_ceil(MIN_SEGMENT);
    let n = u64::from(connections.max(1)).min(by_size).max(1);
    let base = total / n;
    let extra = total % n;
    let mut out = Vec::with_capacity(n as usize);
    let mut start = 0;
    for i in 0..n {
        // The first `extra` segments take one more byte.
        let len = base + u64::from(i < extra);
        out.push(Segment {
            idx: i as u32,
            start,
            end: start + len - 1,
            downloaded: 0,
        });
        start += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_covers(total: u64, segs: &[Segment]) {
        assert_eq!(segs.first().map(|s| s.start), Some(0));
        assert_eq!(segs.last().map(|s| s.end), Some(total - 1));
        for w in segs.windows(2) {
            assert_eq!(w[0].end + 1, w[1].start, "gap or overlap");
        }
        assert_eq!(segs.iter().map(|s| s.len()).sum::<u64>(), total);
    }

    #[test]
    fn empty_file_has_no_segments() {
        assert!(plan(0, 4).is_empty());
    }

    #[test]
    fn small_file_uses_single_segment() {
        let s = plan(1000, 8);
        assert_eq!(s.len(), 1);
        assert_covers(1000, &s);
    }

    #[test]
    fn large_file_uses_all_connections() {
        let total = 100 * 1024 * 1024 + 3;
        let s = plan(total, 4);
        assert_eq!(s.len(), 4);
        assert_covers(total, &s);
        let lens: Vec<u64> = s.iter().map(|s| s.len()).collect();
        assert!(lens.iter().max().unwrap_or(&0) - lens.iter().min().unwrap_or(&0) <= 1);
    }

    #[test]
    fn segment_count_is_capped_by_size() {
        let total = MIN_SEGMENT * 2;
        let s = plan(total, 16);
        assert_eq!(s.len(), 2);
        assert_covers(total, &s);
    }

    #[test]
    fn zero_connections_behaves_like_one() {
        assert_eq!(plan(5_000_000, 0).len(), 1);
    }

    #[test]
    fn indexes_are_sequential() {
        let s = plan(10 * MIN_SEGMENT, 5);
        let idx: Vec<u32> = s.iter().map(|s| s.idx).collect();
        assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    }
}
