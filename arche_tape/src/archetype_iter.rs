pub use bitset_iterator::BitsetIterator;

mod bitset_iterator {

    use std::{borrow::BorrowMut, marker::PhantomData, slice::Iter};
    pub struct BitsetIterator<'a, Iters: BorrowMut<[(Iter<'a, usize>, fn(usize) -> usize)]>> {
        phantom: PhantomData<&'a [usize]>,
        iters: Iters,

        bit_length: u32,
        index: usize,

        bits_remaining: u32,
        current_bits: usize,
    }

    impl<'a, Iters: BorrowMut<[(Iter<'a, usize>, fn(usize) -> usize)]>> BitsetIterator<'a, Iters> {
        pub(crate) fn new(iters: Iters, bit_length: u32) -> Self {
            Self {
                phantom: PhantomData,
                iters,

                bit_length,
                index: 0,

                bits_remaining: 0,
                current_bits: 0,
            }
        }
    }

    impl<'a, Iters: BorrowMut<[(Iter<'a, usize>, fn(usize) -> usize)]>> Iterator
        for BitsetIterator<'a, Iters>
    {
        type Item = usize;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if self.bits_remaining == 0 {
                    // We have to initialise filtered to a proper value so we hand write the first iteration of the loop :(
                    let mut iter = self.iters.borrow_mut().iter_mut();
                    let (first_iter, first_map) = iter.next()?;
                    let mut filtered: usize = first_map(*first_iter.next()?);

                    for (iter, map) in iter {
                        filtered &= map(*iter.next()?);
                    }

                    self.bits_remaining = usize::BITS;
                    self.current_bits = filtered;
                }

                let zeros = self.current_bits.trailing_zeros();

                // Right shifting leaves zeros in its place so we know we've run out of ones when we hit usize::BITS Number of zeros
                if zeros == usize::BITS {
                    self.index += self.bits_remaining as usize;
                    self.bits_remaining = 0;
                    continue;
                }

                self.bits_remaining -= zeros + 1;
                self.current_bits >>= zeros + 1;
                self.index += zeros as usize + 1;

                if self.index > self.bit_length as usize {
                    // Hack but make sure that calling next() after None is returned continues to return None by
                    // setting an empty iterator and making the subsequent Next calls try and call next() on it
                    self.current_bits = 0;
                    // Indexing by 0 should always be fine since an empty slice will never make it this far
                    self.iters.borrow_mut()[0].0 = [].iter();
                    return None;
                }

                return Some(self.index - 1);
            }
        }
    }
}

pub struct Bitvec {
    pub(crate) data: Vec<usize>,
    /// Length in bits of the bitvec
    pub(crate) len: usize,
}

impl Bitvec {
    pub(crate) fn new() -> Self {
        Self {
            data: Vec::new(),
            len: 0,
        }
    }

    pub(crate) fn with_capacity(bit_cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(bit_cap / usize::BITS as usize),
            len: 0,
        }
    }

    pub(crate) fn get_bit(&self, index: usize) -> Option<bool> {
        if index >= self.len {
            return None;
        }

        let data_idx = index / usize::BITS as usize;
        let bit_idx = index % usize::BITS as usize;

        Some(((self.data[data_idx] >> bit_idx) & 1) == 1)
    }

    pub(crate) fn set_bit(&mut self, index: usize, value: bool) {
        let data_idx = index / usize::BITS as usize;
        let bit_idx = index % usize::BITS as usize;

        if index >= self.len {
            self.len = index + 1;

            if self.data.len() < data_idx + 1 {
                self.data.resize_with(data_idx + 1, || 0);
            }
        }

        let bits = &mut self.data[data_idx];
        let mask = 1 << bit_idx;
        *bits &= !mask;
        *bits |= (value as usize) << bit_idx;
    }
}

pub struct Bitsetsss {
    bitsets: Vec<Option<Bitvec>>,
}

use crate::EcsId;
impl Bitsetsss {
    pub(crate) fn new() -> Self {
        Self {
            bitsets: Vec::new(),
        }
    }

    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            bitsets: Vec::with_capacity(cap),
        }
    }

    pub(crate) fn insert_bitvec(&mut self, comp_id: EcsId) {
        if let None = self.bitsets.get(comp_id.uindex()) {
            self.bitsets
                .resize_with(comp_id.uindex() + 1, || Some(Bitvec::new()));
            return;
        }

        if let bitset @ None = &mut self.bitsets[comp_id.uindex()] {
            *bitset = Some(Bitvec::new());
            return;
        }

        panic!("Attempted to insert a bitvec that already existed")
    }

    pub(crate) fn get_bitvec(&self, comp_id: EcsId) -> Option<&Bitvec> {
        self.bitsets.get(comp_id.uindex())?.as_ref()
    }

    pub(crate) fn set_bit(&mut self, entity: EcsId, index: usize, value: bool) {
        let bitvec = (&mut self.bitsets[entity.uindex()])
            .get_or_insert_with(|| Bitvec::with_capacity(index));
        bitvec.set_bit(index, value);
    }
}
