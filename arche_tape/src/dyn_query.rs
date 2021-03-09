use crate::utils::EitherGuard;
use crate::{world::Archetype, EcsId, World};
use std::{convert::TryInto, marker::PhantomData, mem, ptr};

struct IntraArchetypeIter<'a, const N: usize> {
    remaining: usize,

    ptrs: [*mut u8; N],
    offsets: [usize; N],

    phantom: PhantomData<&'a mut Archetype>,
}

impl<'a, const N: usize> IntraArchetypeIter<'a, N> {
    /// Empty iterator
    fn unit() -> Self {
        Self {
            remaining: 0,
            ptrs: [0x0 as _; N],
            offsets: [0; N],
            phantom: PhantomData,
        }
    }

    fn new(length: usize, ptrs: [*mut u8; N], offsets: [usize; N]) -> Self {
        Self {
            remaining: length,
            ptrs,
            offsets,
            phantom: PhantomData,
        }
    }
}

impl<'a, const N: usize> Iterator for IntraArchetypeIter<'a, N> {
    type Item = [*mut u8; N];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let ptrs = self.ptrs;

        for (ptr, offset) in self.ptrs.iter_mut().zip(self.offsets.iter()) {
            unsafe { *ptr = ptr.add(*offset) }
        }
        self.remaining -= 1;

        Some(ptrs)
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PtrLen(pub *mut u8, pub usize);

pub struct DynQueryColumnIter<'a, const N: usize> {
    comp_ids: [Option<EcsId>; N],
    create_ptr: [fn(&Archetype, Option<EcsId>) -> (*mut u8, usize); N],
    archetype_iter: crate::world::ArchetypeIter<'a, N>,
}

impl<'a, const N: usize> Iterator for DynQueryColumnIter<'a, N> {
    type Item = [PtrLen; N];

    fn next(&mut self) -> Option<Self::Item> {
        let archetype = self.archetype_iter.next()?;
        let mut ptrs = [PtrLen(0x0 as _, archetype.entities.len()); N];
        for n in 0..N {
            let (ptr, _) = self.create_ptr[n](archetype, self.comp_ids[n]);
            ptrs[n].0 = ptr;
        }
        Some(ptrs)
    }
}

pub struct DynQueryIter<'a, const N: usize> {
    comp_ids: [Option<EcsId>; N],
    create_ptr: [fn(&Archetype, Option<EcsId>) -> (*mut u8, usize); N],
    archetype_iter: crate::world::ArchetypeIter<'a, N>,
    intra_iter: IntraArchetypeIter<'a, N>,
}

impl<'a, const N: usize> Iterator for DynQueryIter<'a, N> {
    type Item = [*mut u8; N];

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.intra_iter.next() {
                None => {
                    let archetype = self.archetype_iter.next()?;

                    let mut ptrs = [0x0 as _; N];
                    let mut offsets = [0; N];
                    for n in 0..N {
                        let (ptr, offset) = self.create_ptr[n](archetype, self.comp_ids[n]);
                        ptrs[n] = ptr;
                        offsets[n] = offset;
                    }

                    self.intra_iter =
                        IntraArchetypeIter::new(archetype.entities.len(), ptrs, offsets);
                }
                ptrs @ Some(_) => return ptrs,
            }
        }
    }
}

// FFI compatible C interface using a union and a u8 tag
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum FetchType {
    EcsId,
    Mut(EcsId),
    Immut(EcsId),
}

impl FetchType {
    pub(crate) fn get_id(&self) -> Option<EcsId> {
        Some(match self {
            &Self::Mut(id) | &Self::Immut(id) => id,
            Self::EcsId => return None,
        })
    }

    pub(crate) fn make_create_ptr_fn(&self) -> fn(&Archetype, Option<EcsId>) -> (*mut u8, usize) {
        match self {
            FetchType::EcsId => |archetype, _| {
                (
                    archetype.entities.as_ptr() as *mut EcsId as *mut u8,
                    core::mem::size_of::<EcsId>(),
                )
            },
            FetchType::Immut(_) => |archetype, id| {
                let storage_idx = archetype.comp_lookup[&id.unwrap()];
                let storage = unsafe { &*archetype.component_storages[storage_idx].1.get() };
                let size = storage.get_type_info().layout.size();
                (unsafe { storage.as_immut_ptr() as *mut u8 }, size)
            },
            FetchType::Mut(_) => |archetype, id| {
                let storage_idx = archetype.comp_lookup[&id.unwrap()];
                let storage = unsafe { &mut *archetype.component_storages[storage_idx].1.get() };
                let size = storage.get_type_info().layout.size();
                (unsafe { storage.as_mut_ptr() }, size)
            },
        }
    }
}

pub struct DynQuery<'a, const N: usize> {
    world: &'a World,
    _guards: [EitherGuard<'a>; N],
    fetches: [FetchType; N],

    /// If set to true it means that some of the EcsId's used were not alive/existing
    incomplete: bool,
}

impl<'a, const N: usize> DynQuery<'a, N> {
    pub(crate) fn new(world: &'a World, fetches: [FetchType; N]) -> Self {
        let mut incomplete = false;

        const NONE: EitherGuard = EitherGuard::None;
        let mut guards = [NONE; N];

        for (fetch, guard) in fetches.iter().zip(guards.iter_mut()) {
            let ecs_id = match fetch {
                FetchType::EcsId => continue,
                FetchType::Immut(id) | FetchType::Mut(id) => id,
            };

            if let Some(&idx) = world.lock_lookup.get(ecs_id) {
                let lock = &world.locks[idx];
                match fetch {
                    FetchType::Mut(_) => *guard = EitherGuard::Write(lock.write().unwrap()),
                    FetchType::Immut(_) => *guard = EitherGuard::Read(lock.read().unwrap()),
                    _ => (),
                }
            } else {
                incomplete = true;
            }
        }

        Self {
            world,
            _guards: guards,
            fetches,
            incomplete,
        }
    }

    pub fn column_iter(&mut self) -> DynQueryColumnIter<'_, N> {
        const NONE_ID: Option<EcsId> = None;
        let mut ecs_ids = [NONE_ID; N];
        for (fetch, ecs_id) in self.fetches.iter().zip(ecs_ids.iter_mut()) {
            if let FetchType::Immut(id) | FetchType::Mut(id) = fetch {
                *ecs_id = Some(*id);
            }
        }

        const DEFAULT_FN: fn(&Archetype, Option<EcsId>) -> (*mut u8, usize) = |_, _| panic!();
        let mut create_ptr = [DEFAULT_FN; N];
        for (fetch, func) in self.fetches.iter().zip(create_ptr.iter_mut()) {
            *func = fetch.make_create_ptr_fn();
        }

        let archetype_iter = if self.incomplete {
            let bit_length = 0;
            let neg_fn: fn(_) -> _ = |x: usize| !x;

            let iters: Box<[_; N]> = vec![(self.world.entities_bitvec.data.iter(), neg_fn); N]
                .into_boxed_slice()
                .try_into()
                .unwrap();
            let iters = *iters;

            self.world.query_archetypes(iters, bit_length)
        } else {
            let identity_fn: fn(_) -> _ = |x| x;

            let mut bit_length = self.world.entities_bitvec.len as u32;
            let boxed_iters = ecs_ids
                .iter()
                .map(|id| match id {
                    None => (self.world.entities_bitvec.data.iter(), identity_fn),
                    Some(id) => {
                        let bitvec = self.world.archetype_bitset.get_bitvec(*id).unwrap();
                        if { bitvec.len as u32 } < bit_length {
                            bit_length = bitvec.len as u32;
                        }

                        (bitvec.data.iter(), identity_fn)
                    }
                })
                .collect::<Box<[_]>>();
            let iters: Box<[_; N]> = boxed_iters.try_into().unwrap();
            let iters = *iters;

            self.world.query_archetypes(iters, bit_length)
        };

        DynQueryColumnIter {
            comp_ids: ecs_ids,
            create_ptr,
            archetype_iter,
        }
    }

    pub fn iter(&mut self) -> DynQueryIter<'_, N> {
        const NONE_ID: Option<EcsId> = None;
        let mut ecs_ids = [NONE_ID; N];
        for (fetch, ecs_id) in self.fetches.iter().zip(ecs_ids.iter_mut()) {
            if let FetchType::Immut(id) | FetchType::Mut(id) = fetch {
                *ecs_id = Some(*id);
            }
        }

        const DEFAULT_FN: fn(&Archetype, Option<EcsId>) -> (*mut u8, usize) = |_, _| panic!();
        let mut create_ptr = [DEFAULT_FN; N];
        for (fetch, func) in self.fetches.iter().zip(create_ptr.iter_mut()) {
            *func = fetch.make_create_ptr_fn();
        }

        let archetype_iter = if self.incomplete {
            let bit_length = 0;
            let neg_fn: fn(_) -> _ = |x: usize| !x;

            let iters: Box<[_; N]> = vec![(self.world.entities_bitvec.data.iter(), neg_fn); N]
                .into_boxed_slice()
                .try_into()
                .unwrap();
            let iters = *iters;

            self.world.query_archetypes(iters, bit_length)
        } else {
            let identity_fn: fn(_) -> _ = |x| x;

            let mut bit_length = self.world.entities_bitvec.len as u32;
            let boxed_iters = ecs_ids
                .iter()
                .map(|id| match id {
                    None => (self.world.entities_bitvec.data.iter(), identity_fn),
                    Some(id) => {
                        let bitvec = self.world.archetype_bitset.get_bitvec(*id).unwrap();
                        if { bitvec.len as u32 } < bit_length {
                            bit_length = bitvec.len as u32;
                        }

                        (bitvec.data.iter(), identity_fn)
                    }
                })
                .collect::<Box<[_]>>();

            let iters: Box<[_; N]> = boxed_iters.try_into().unwrap();
            let iters = *iters;

            self.world.query_archetypes(iters, bit_length)
        };

        DynQueryIter {
            comp_ids: ecs_ids,
            create_ptr,
            archetype_iter,
            intra_iter: IntraArchetypeIter::unit(),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct FFIDynQuery {
    pub ptr: *mut u8,
    pub len: usize,
}

fn _query_dynamic<const N: usize>(world: &World, fetches: [FetchType; N]) -> FFIDynQuery {
    let ptr = Box::into_raw(Box::new(DynQuery::new(world, fetches))) as *mut u8;
    FFIDynQuery { ptr, len: N }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FFIDynQueryIter {
    pub ptr: *mut u8,
    pub len: usize,
}

fn __dyn_query_iter<const N: usize>(ptr: *mut u8) -> FFIDynQueryIter {
    let mut query = unsafe { Box::from_raw(ptr as *mut DynQuery<N>) };
    let ptr = Box::into_raw(Box::new(query.column_iter())) as *mut u8;
    mem::forget(query);
    FFIDynQueryIter { ptr, len: N }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FFIDynQueryResult {
    pub ptr: *mut u8,
    pub len: usize,
}

fn __dyn_query_next<const N: usize>(ptr: *mut u8) -> FFIDynQueryResult {
    let mut iter = unsafe { Box::from_raw(ptr as *mut DynQueryColumnIter<N>) };
    let ret = if let Some(a) = iter.next() {
        FFIDynQueryResult {
            ptr: Box::into_raw(Box::new(a)) as *mut u8,
            len: N,
        }
    } else {
        FFIDynQueryResult {
            ptr: ptr::null_mut(),
            len: 0,
        }
    };
    mem::forget(iter);
    ret
}

macro_rules! impl_ffi_dyn_query {
    ($($N:literal)*) => {
        #[no_mangle]
        pub unsafe extern "C" fn _dyn_query_new(
            world: &World,
            fetches: *const FetchType,
            len: usize,
        ) -> FFIDynQuery {
            unsafe {
                match len {
                    $(
                        $N => _query_dynamic::<$N>(
                            world,
                            std::slice::from_raw_parts(fetches, $N).try_into().unwrap(),
                        ),
                    )*
                    _ => unimplemented!("Dynamic queries over FFI for {} fetches are not implemented.", len),
                }
            }
        }

        #[no_mangle]
        pub unsafe extern "C" fn _dyn_query_iter(q: FFIDynQuery) -> FFIDynQueryIter {
            match q.len {
                $(
                    $N => __dyn_query_iter::<$N>(q.ptr),
                )*
                _ => unimplemented!("Dynamic queries over FFI for {} fetches are not implemented.", q.len),
            }
        }
        
        #[no_mangle]
        pub unsafe extern "C" fn _dyn_query_next(qi: FFIDynQueryIter) -> FFIDynQueryResult {
            match qi.len {
                $(
                    $N => __dyn_query_next::<$N>(qi.ptr),
                )*
                _ => unimplemented!("Dynamic queries over FFI for {} fetches are not implemented.", qi.len),
            }
        }
    };
}

impl_ffi_dyn_query!(1 2 3 4 5 6 7 8);