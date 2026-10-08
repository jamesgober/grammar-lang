//! Lowering scalar-value ranges to UTF-8 byte-range sequences.
//!
//! The lexer's automaton runs over bytes, so a class like `[α-ω]` must become
//! byte transitions. Any range of scalar values splits into a handful of
//! sequences, each a fixed-length run of byte ranges whose cross product is
//! exactly the encodings of a contiguous sub-range — the construction Russ Cox
//! describes for RE2 and `regex-syntax` uses.

use alloc::vec::Vec;

/// A run of one to four byte ranges; the encodings it matches are every
/// `b0 b1 ... bn` with each byte inside its range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sequence {
    ranges: [(u8, u8); 4],
    len: u8,
}

impl Sequence {
    /// The byte ranges, first byte first.
    #[inline]
    pub(crate) fn ranges(&self) -> &[(u8, u8)] {
        &self.ranges[..self.len as usize]
    }
}

/// Encoding-length boundaries: the last scalar value of each UTF-8 length.
const MAX_BY_LEN: [u32; 3] = [0x7F, 0x7FF, 0xFFFF];

/// Appends the sequences matching exactly the encodings of `lo..=hi` to `out`.
///
/// The range must not contain a surrogate; the class normalizer guarantees it.
pub(crate) fn sequences(lo: u32, hi: u32, out: &mut Vec<Sequence>) {
    let mut stack = Vec::with_capacity(8);
    stack.push((lo, hi));
    'next: while let Some((start, mut end)) = stack.pop() {
        loop {
            // Keep the range inside one encoded length.
            for &max in &MAX_BY_LEN {
                if start <= max && max < end {
                    stack.push((max + 1, end));
                    end = max;
                }
            }
            // Split until every continuation byte spans a full or aligned run.
            let mut split = false;
            for i in 1..4 {
                let mask = (1u32 << (6 * i)) - 1;
                if start & !mask != end & !mask {
                    if start & mask != 0 {
                        stack.push(((start | mask) + 1, end));
                        end = start | mask;
                        split = true;
                        break;
                    }
                    if end & mask != mask {
                        stack.push((end & !mask, end));
                        end = (end & !mask) - 1;
                        split = true;
                        break;
                    }
                }
            }
            if split {
                continue;
            }
            let (Some(a), Some(b)) = (char::from_u32(start), char::from_u32(end)) else {
                continue 'next;
            };
            let mut abuf = [0u8; 4];
            let mut bbuf = [0u8; 4];
            let a = a.encode_utf8(&mut abuf).as_bytes();
            let b = b.encode_utf8(&mut bbuf).as_bytes();
            let mut seq = Sequence {
                ranges: [(0, 0); 4],
                len: a.len() as u8,
            };
            for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
                seq.ranges[i] = (x, y);
            }
            out.push(seq);
            continue 'next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scalar value in `lo..=hi` must be matched by exactly one
    /// sequence, and nothing outside the range by any.
    fn check(lo: u32, hi: u32) {
        let mut seqs = Vec::new();
        sequences(lo, hi, &mut seqs);
        let matches = |bytes: &[u8]| {
            seqs.iter()
                .filter(|s| {
                    s.ranges().len() == bytes.len()
                        && s.ranges()
                            .iter()
                            .zip(bytes)
                            .all(|(&(a, b), &x)| a <= x && x <= b)
                })
                .count()
        };
        let probe_lo = lo.saturating_sub(300);
        let probe_hi = (hi + 300).min(0x10FFFF);
        for v in probe_lo..=probe_hi {
            let Some(c) = char::from_u32(v) else { continue };
            let mut buf = [0u8; 4];
            let enc = c.encode_utf8(&mut buf).as_bytes();
            let expected = usize::from((lo..=hi).contains(&v));
            assert_eq!(matches(enc), expected, "U+{v:04X} in {lo:X}..={hi:X}");
        }
    }

    #[test]
    fn ascii_is_one_range() {
        let mut seqs = Vec::new();
        sequences(0x61, 0x7A, &mut seqs);
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].ranges(), &[(0x61, 0x7A)]);
    }

    #[test]
    fn ranges_cover_exactly() {
        check(0, 0x7F);
        check(0x41, 0x3A9);
        check(0x7F0, 0x810);
        check(0xFFF0, 0x1_0010);
        check(0x3B1, 0x3C9);
        check(0x1F600, 0x1F64F);
        check(0xE000, 0xE100);
        check(0x80, 0xD7FF);
        check(0x10_FF00, 0x10_FFFF);
    }

    #[test]
    fn the_full_range_needs_few_sequences() {
        let mut seqs = Vec::new();
        sequences(0, 0xD7FF, &mut seqs);
        sequences(0xE000, 0x10_FFFF, &mut seqs);
        assert!(seqs.len() <= 12, "{} sequences", seqs.len());
    }
}
