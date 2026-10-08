//! Two small data structures the generators share: a matrix of equal-width
//! bitsets, and an interning table that numbers distinct `u32` slices.

use alloc::{vec, vec::Vec};

/// Rows of equal-width bitsets stored in one contiguous buffer.
///
/// The LALR(1) lookahead computation keeps one terminal set per nonterminal
/// transition and unions them along relation edges; packing the rows together
/// keeps those unions to straight-line word loops over adjacent memory.
#[derive(Clone, Debug)]
pub(crate) struct BitMatrix {
    words: usize,
    data: Vec<u64>,
}

impl BitMatrix {
    /// A matrix of `rows` empty sets, each able to hold bits `0..bits`.
    pub(crate) fn new(rows: usize, bits: usize) -> Self {
        let words = bits.div_ceil(64);
        Self {
            words,
            data: vec![0; rows * words],
        }
    }

    /// The words of row `row`.
    #[inline]
    pub(crate) fn row(&self, row: usize) -> &[u64] {
        &self.data[row * self.words..(row + 1) * self.words]
    }

    /// Adds `bit` to row `row`.
    #[inline]
    pub(crate) fn insert(&mut self, row: usize, bit: usize) {
        self.data[row * self.words + bit / 64] |= 1 << (bit % 64);
    }

    /// Whether row `row` holds `bit`.
    #[inline]
    pub(crate) fn contains(&self, row: usize, bit: usize) -> bool {
        self.data[row * self.words + bit / 64] & (1 << (bit % 64)) != 0
    }

    /// Clears every bit of row `row`.
    #[inline]
    pub(crate) fn clear_row(&mut self, row: usize) {
        let words = self.words;
        self.data[row * words..(row + 1) * words].fill(0);
    }

    /// Unions row `src` into row `dst`.
    #[inline]
    pub(crate) fn union_rows(&mut self, dst: usize, src: usize) {
        if dst == src {
            return;
        }
        let words = self.words;
        let (dst, src) = if dst < src {
            let (head, tail) = self.data.split_at_mut(src * words);
            (&mut head[dst * words..(dst + 1) * words], &tail[..words])
        } else {
            let (head, tail) = self.data.split_at_mut(dst * words);
            (&mut tail[..words], &head[src * words..(src + 1) * words])
        };
        for (d, s) in dst.iter_mut().zip(src) {
            *d |= *s;
        }
    }

    /// Unions `src` (a row-width word slice) into row `dst`.
    #[inline]
    pub(crate) fn union_from(&mut self, dst: usize, src: &[u64]) {
        let words = self.words;
        for (d, s) in self.data[dst * words..(dst + 1) * words]
            .iter_mut()
            .zip(src)
        {
            *d |= *s;
        }
    }

    /// Overwrites row `dst` with row `src`.
    #[inline]
    pub(crate) fn copy_row(&mut self, dst: usize, src: usize) {
        let words = self.words;
        self.data
            .copy_within(src * words..(src + 1) * words, dst * words);
    }
}

/// Iterates the set bits of a word slice in ascending order.
#[inline]
pub(crate) fn bits(words: &[u64]) -> impl Iterator<Item = usize> + '_ {
    words.iter().enumerate().flat_map(|(w, &word)| {
        let mut rest = word;
        core::iter::from_fn(move || {
            if rest == 0 {
                return None;
            }
            let bit = rest.trailing_zeros() as usize;
            rest &= rest - 1;
            Some(w * 64 + bit)
        })
    })
}

/// Numbers distinct `u32` slices in insertion order.
///
/// Subset construction and the LR(0) automaton both discover states as sorted
/// sets of smaller things — NFA states, LR items — and must recognise a set
/// they have seen before. The keys live back to back in one buffer; an
/// open-addressed index of `id + 1` entries (0 marks a free slot) finds them by
/// hash, so a lookup is one hash, a probe or two, and a slice compare.
#[derive(Debug)]
pub(crate) struct SliceMap {
    data: Vec<u32>,
    ends: Vec<usize>,
    hashes: Vec<u64>,
    index: Vec<u32>,
}

impl SliceMap {
    /// An empty map.
    pub(crate) fn new() -> Self {
        Self {
            data: Vec::new(),
            ends: Vec::new(),
            hashes: Vec::new(),
            index: vec![0; 64],
        }
    }

    /// The number of distinct slices stored.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    /// The total length of every stored slice.
    #[inline]
    pub(crate) fn words(&self) -> usize {
        self.data.len()
    }

    /// The slice numbered `id`.
    #[inline]
    pub(crate) fn get(&self, id: usize) -> &[u32] {
        let start = if id == 0 { 0 } else { self.ends[id - 1] };
        &self.data[start..self.ends[id]]
    }

    /// The number of `key`, inserting it first if it is new. The flag is true
    /// when the key was inserted.
    pub(crate) fn insert(&mut self, key: &[u32]) -> (u32, bool) {
        let hash = hash(key);
        let mask = self.index.len() - 1;
        let mut slot = (hash as usize) & mask;
        loop {
            match self.index[slot] {
                0 => break,
                entry => {
                    let id = (entry - 1) as usize;
                    if self.hashes[id] == hash && self.get(id) == key {
                        return (entry - 1, false);
                    }
                }
            }
            slot = (slot + 1) & mask;
        }
        let id = self.ends.len() as u32;
        self.data.extend_from_slice(key);
        self.ends.push(self.data.len());
        self.hashes.push(hash);
        self.index[slot] = id + 1;
        if self.ends.len() * 2 > self.index.len() {
            self.grow();
        }
        (id, true)
    }

    /// Doubles the index and re-places every entry.
    fn grow(&mut self) {
        let size = self.index.len() * 2;
        let mask = size - 1;
        let mut index = vec![0u32; size];
        for (id, &hash) in self.hashes.iter().enumerate() {
            let mut slot = (hash as usize) & mask;
            while index[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            index[slot] = id as u32 + 1;
        }
        self.index = index;
    }
}

/// A fast, non-cryptographic hash of a `u32` slice (the Fx multiply-rotate
/// scheme), finished with a fold so the low bits used for slot selection see
/// every input word.
#[inline]
fn hash(key: &[u32]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h = key.len() as u64;
    for &word in key {
        h = (h.rotate_left(5) ^ u64::from(word)).wrapping_mul(K);
    }
    h ^ (h >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_map_numbers_distinct_slices() {
        let mut map = SliceMap::new();
        assert_eq!(map.insert(&[1, 2, 3]), (0, true));
        assert_eq!(map.insert(&[]), (1, true));
        assert_eq!(map.insert(&[1, 2, 3]), (0, false));
        assert_eq!(map.insert(&[]), (1, false));
        assert_eq!(map.get(0), &[1, 2, 3]);
        assert_eq!(map.get(1), &[] as &[u32]);
        for i in 0..1000u32 {
            let (id, new) = map.insert(&[i, i + 1]);
            assert!(new);
            assert_eq!(id, i + 2);
        }
        for i in 0..1000u32 {
            assert_eq!(map.insert(&[i, i + 1]), (i + 2, false));
        }
        assert_eq!(map.len(), 1002);
    }

    #[test]
    fn bit_matrix_rows_are_independent() {
        let mut m = BitMatrix::new(3, 130);
        m.insert(0, 0);
        m.insert(0, 129);
        m.insert(2, 64);
        m.union_rows(1, 0);
        m.union_rows(1, 2);
        assert_eq!(bits(m.row(1)).collect::<Vec<_>>(), [0, 64, 129]);
        m.union_rows(2, 1);
        assert!(m.contains(2, 129));
        m.copy_row(0, 2);
        assert_eq!(bits(m.row(0)).collect::<Vec<_>>(), [0, 64, 129]);
        m.clear_row(0);
        assert_eq!(bits(m.row(0)).count(), 0);
    }
}
