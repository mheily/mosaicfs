//! Block map for sparse cache entries.
//!
//! Tracks which byte ranges of a file have been cached, using a sorted
//! `Vec<Range<u64>>` of present intervals. Supports presence checks,
//! missing range calculation, and interval insertion with merge.

use std::ops::Range;

/// A block map tracking which byte ranges are present in the cache.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockMap {
    /// Sorted, non-overlapping, non-adjacent intervals of present bytes.
    intervals: Vec<Range<u64>>,
}

impl BlockMap {
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
        }
    }

    /// Deserialize from a binary blob (pairs of little-endian u64s).
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut intervals = Vec::new();
        let mut offset = 0;
        while offset + 16 <= data.len() {
            let start = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            let end = u64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap());
            if start < end {
                intervals.push(start..end);
            }
            offset += 16;
        }
        Self { intervals }
    }

    /// Serialize to a binary blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(self.intervals.len() * 16);
        for range in &self.intervals {
            data.extend_from_slice(&range.start.to_le_bytes());
            data.extend_from_slice(&range.end.to_le_bytes());
        }
        data
    }

    /// Check if a byte offset is present (binary search).
    pub fn contains(&self, offset: u64) -> bool {
        self.intervals
            .binary_search_by(|range| {
                if offset < range.start {
                    std::cmp::Ordering::Greater
                } else if offset >= range.end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// Check if an entire range is present.
    pub fn range_present(&self, query: &Range<u64>) -> bool {
        if query.start >= query.end {
            return true;
        }
        for range in &self.intervals {
            if range.start <= query.start && range.end >= query.end {
                return true;
            }
        }
        false
    }

    /// Calculate missing sub-ranges within a requested range.
    pub fn missing_ranges(&self, query: &Range<u64>) -> Vec<Range<u64>> {
        if query.start >= query.end {
            return vec![];
        }
        let mut missing = Vec::new();
        let mut cursor = query.start;

        for range in &self.intervals {
            if range.start >= query.end {
                break;
            }
            if range.end <= cursor {
                continue;
            }
            if range.start > cursor {
                missing.push(cursor..range.start.min(query.end));
            }
            cursor = cursor.max(range.end);
        }

        if cursor < query.end {
            missing.push(cursor..query.end);
        }
        missing
    }

    /// Insert a range, merging with adjacent/overlapping intervals.
    pub fn insert(&mut self, new: Range<u64>) {
        if new.start >= new.end {
            return;
        }

        let mut merged_start = new.start;
        let mut merged_end = new.end;
        let mut first = self.intervals.len();
        let mut last = 0;

        for (i, range) in self.intervals.iter().enumerate() {
            // Check if this interval overlaps or is adjacent to the new range
            if range.end < merged_start || range.start > merged_end {
                continue;
            }
            if i < first {
                first = i;
            }
            last = i + 1;
            merged_start = merged_start.min(range.start);
            merged_end = merged_end.max(range.end);
        }

        if first >= last {
            // No overlap — insert at sorted position
            let pos = self
                .intervals
                .binary_search_by(|r| r.start.cmp(&new.start))
                .unwrap_or_else(|p| p);
            self.intervals.insert(pos, new);
        } else {
            // Replace overlapping range(s) with merged interval
            self.intervals.drain(first..last);
            self.intervals.insert(first, merged_start..merged_end);
        }
    }

    /// Number of distinct intervals (for fragmentation monitoring).
    pub fn interval_count(&self) -> usize {
        self.intervals.len()
    }

    /// Total cached bytes.
    pub fn cached_bytes(&self) -> u64 {
        self.intervals.iter().map(|r| r.end - r.start).sum()
    }

    /// Coalesce adjacent missing sub-ranges within `query` into fewer requests.
    /// Returns the missing ranges, but merges adjacent ones that are closer
    /// than `min_gap` bytes apart.
    pub fn coalesced_missing(&self, query: &Range<u64>, min_gap: u64) -> Vec<Range<u64>> {
        let missing = self.missing_ranges(query);
        if missing.len() <= 1 {
            return missing;
        }
        let mut coalesced = Vec::new();
        let mut current = missing[0].clone();
        for m in &missing[1..] {
            if m.start - current.end <= min_gap {
                current.end = m.end;
            } else {
                coalesced.push(current.clone());
                current = m.clone();
            }
        }
        coalesced.push(current);
        coalesced
    }
}

impl Default for BlockMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_block_map() {
        let bm = BlockMap::new();
        assert!(!bm.contains(0));
        assert!(!bm.contains(100));
        assert_eq!(bm.interval_count(), 0);
        assert_eq!(bm.cached_bytes(), 0);
    }

    #[test]
    fn test_insert_single() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        assert!(bm.contains(100));
        assert!(bm.contains(199));
        assert!(!bm.contains(200));
        assert!(!bm.contains(99));
        assert_eq!(bm.interval_count(), 1);
        assert_eq!(bm.cached_bytes(), 100);
    }

    #[test]
    fn test_insert_non_overlapping() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(300..400);
        assert_eq!(bm.interval_count(), 2);
        assert!(bm.contains(150));
        assert!(bm.contains(350));
        assert!(!bm.contains(250));
    }

    #[test]
    fn test_insert_overlapping_merge() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(150..300);
        assert_eq!(bm.interval_count(), 1);
        assert_eq!(bm.cached_bytes(), 200);
        assert!(bm.contains(100));
        assert!(bm.contains(250));
    }

    #[test]
    fn test_insert_adjacent_merge() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(200..300);
        assert_eq!(bm.interval_count(), 1);
        assert_eq!(bm.cached_bytes(), 200);
    }

    #[test]
    fn test_insert_merge_multiple() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(300..400);
        bm.insert(500..600);
        assert_eq!(bm.interval_count(), 3);
        // Insert range spanning first two
        bm.insert(150..350);
        assert_eq!(bm.interval_count(), 2);
        assert!(bm.range_present(&(100..400)));
    }

    #[test]
    fn test_insert_superset() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(300..400);
        bm.insert(50..500);
        assert_eq!(bm.interval_count(), 1);
        assert_eq!(bm.cached_bytes(), 450);
    }

    #[test]
    fn test_range_present() {
        let mut bm = BlockMap::new();
        bm.insert(100..300);
        assert!(bm.range_present(&(100..200)));
        assert!(bm.range_present(&(100..300)));
        assert!(!bm.range_present(&(100..301)));
        assert!(!bm.range_present(&(99..300)));
    }

    #[test]
    fn test_missing_ranges() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(300..400);

        let missing = bm.missing_ranges(&(0..500));
        assert_eq!(missing, vec![0..100, 200..300, 400..500]);
    }

    #[test]
    fn test_missing_ranges_no_gaps() {
        let mut bm = BlockMap::new();
        bm.insert(0..500);
        let missing = bm.missing_ranges(&(0..500));
        assert!(missing.is_empty());
    }

    #[test]
    fn test_missing_ranges_all_missing() {
        let bm = BlockMap::new();
        let missing = bm.missing_ranges(&(0..1000));
        assert_eq!(missing, vec![0..1000]);
    }

    #[test]
    fn test_serialization_round_trip() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(300..400);
        bm.insert(500..600);

        let bytes = bm.to_bytes();
        let bm2 = BlockMap::from_bytes(&bytes);
        assert_eq!(bm, bm2);
    }

    #[test]
    fn test_empty_serialization() {
        let bm = BlockMap::new();
        let bytes = bm.to_bytes();
        assert!(bytes.is_empty());
        let bm2 = BlockMap::from_bytes(&bytes);
        assert_eq!(bm, bm2);
    }

    #[test]
    fn test_coalesced_missing() {
        let mut bm = BlockMap::new();
        bm.insert(100..200);
        bm.insert(210..300);
        // Gap between 200..210 is only 10 bytes
        let coalesced = bm.coalesced_missing(&(0..500), 20);
        // First missing: 0..100, then 200..210 is close to 300..500
        // Actually: missing = [0..100, 200..210, 300..500]
        // 200..210 and 300..500 have a gap of 90, so only 0..100 and 200..210 might merge
        // 0..100 ends at 100, 200..210 starts at 200 -> gap=100 > 20, no merge
        // 200..210 ends at 210, 300..500 starts at 300 -> gap=90 > 20, no merge
        assert_eq!(coalesced, vec![0..100, 200..210, 300..500]);

        // With a larger min_gap that merges all three
        let coalesced = bm.coalesced_missing(&(0..500), 100);
        // 0..100 to 200..210: gap=100 <= 100, merge -> 0..210
        // 0..210 to 300..500: gap=90 <= 100, merge -> 0..500
        assert_eq!(coalesced, vec![0..500]);
    }

    #[test]
    fn test_insert_empty_range() {
        let mut bm = BlockMap::new();
        bm.insert(100..100);
        assert_eq!(bm.interval_count(), 0);
    }

    #[test]
    fn test_fragmentation_guard_check() {
        let mut bm = BlockMap::new();
        for i in 0..1001 {
            bm.insert((i * 10)..(i * 10 + 5));
        }
        assert!(bm.interval_count() > 1000);
    }

    #[test]
    fn test_contains_binary_search() {
        let mut bm = BlockMap::new();
        // Add many non-adjacent intervals to exercise binary search
        for i in 0..100 {
            bm.insert((i * 100)..(i * 100 + 50));
        }
        assert!(bm.contains(0));
        assert!(bm.contains(49));
        assert!(!bm.contains(50));
        assert!(bm.contains(100));
        assert!(bm.contains(9900));
        assert!(!bm.contains(9950));
    }
}
