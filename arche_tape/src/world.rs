use super::entities::{EcsId, Entities};
use crate::{
    array_vec::ArrayVec,
    bitset_iterator::{BitsetIterator, Bitsetsss, Bitvec},
    dyn_query::{DynQuery, FetchType},
    static_query::StaticQuery,
    Component,
};
use std::{
    any::TypeId,
    cell::UnsafeCell,
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    process::abort,
    ptr,
    slice::Iter,
    sync::RwLock,
};
use untyped_vec::UntypedVec;

pub struct ArchetypeIter<'a, const N: usize> {
    archetypes: &'a [Archetype],
    iter: BitsetIterator<'a, [(Iter<'a, usize>, fn(usize) -> usize); N]>,
}

impl<'a, const N: usize> Iterator for ArchetypeIter<'a, N> {
    type Item = &'a Archetype;

    fn next(&mut self) -> Option<&'a Archetype> {
        self.iter.next().map(|idx| &self.archetypes[idx])
    }
}

const CACHE_SIZE: usize = 4;
pub struct AddRemoveCache {
    cache: ArrayVec<(EcsId, usize), CACHE_SIZE>,
    lookup: HashMap<EcsId, usize, crate::utils::TypeIdHasherBuilder>,
}

impl AddRemoveCache {
    pub(crate) fn new() -> Self {
        Self {
            cache: ArrayVec::new(),
            lookup: HashMap::with_capacity_and_hasher(16, crate::utils::TypeIdHasherBuilder()),
        }
    }

    pub fn lookup_id(&mut self, component_id: EcsId) -> Option<usize> {
        for (id, idx) in self.cache.as_slice() {
            if *id == component_id {
                return Some(*idx);
            }
        }

        if let Some(idx) = self.lookup.get(&component_id) {
            self.cache.push_start((component_id, *idx));
            return Some(*idx);
        }

        None
    }

    pub fn insert_id(&mut self, component_id: EcsId, archetype: usize) {
        self.cache.push_start((component_id, archetype));
        self.lookup.insert(component_id, archetype);
    }
}
pub struct Archetype {
    /// A lookup of a component's TypeId to the index into component_storages/type_ids
    pub(crate) comp_lookup: HashMap<EcsId, usize, crate::utils::TypeIdHasherBuilder>,

    /// This vec effectively acts like a component strage and as such should have its elements ordered the same as a component in component_storages
    pub(crate) entities: Vec<EcsId>,

    /// Component storages are sorted such that lower type_ids are first, this means that when adding/removing components we dont need to
    /// go through the lookup hashmap on the other archetype, we can just zip two iterators over component storages and skip the index
    /// for the removed/added type
    pub(crate) component_storages: Vec<(EcsId, UnsafeCell<UntypedVec>)>, // We need the EcsId here so that we can sort the vec :( the EcsId here should be the same as the one in comp_ids at the same index

    /// The order of this vec is guaranteed to be the same as the order of component storages,
    /// this means that you can .iter().position(|id| ...) to find the index in component_storages for an EcsId
    pub(crate) comp_ids: Vec<EcsId>,

    pub(crate) add_remove_cache: AddRemoveCache,
}

impl Archetype {
    pub fn from_archetype(from: &mut Archetype) -> Archetype {
        Archetype {
            comp_lookup: from.comp_lookup.clone(),
            comp_ids: from.comp_ids.clone(),

            entities: Vec::new(),
            component_storages: {
                // Capacity + 1 incase this gets fed into a from_archetype_with call
                let mut storages = Vec::with_capacity(from.component_storages.len() + 1);
                for storage in from.component_storages.iter_mut() {
                    let untyped_vec = UntypedVec::new_from_untyped_vec(storage.1.get_mut());
                    storages.push((storage.0, UnsafeCell::new(untyped_vec)));
                }
                storages
            },
            add_remove_cache: AddRemoveCache::new(),
        }
    }

    /// # Safety
    ///
    ///    ``with_type_info`` must be valid and correspond to ``with_id``
    #[allow(unused_unsafe)]
    pub unsafe fn from_archetype_with(
        from: &mut Archetype,
        with_type_info: untyped_vec::TypeInfo,
        with_id: EcsId,
    ) -> Archetype {
        let mut new_archetype = Archetype::from_archetype(from);

        assert!(new_archetype.comp_lookup.get(&with_id).is_none());

        new_archetype.comp_ids.push(with_id);
        new_archetype.component_storages.push((
            with_id,
            UnsafeCell::new(unsafe { UntypedVec::new_from_raw(with_type_info) }),
        ));

        // TODO there's no need to sort twice they should have the same ordering
        new_archetype.comp_ids.sort();
        new_archetype
            .component_storages
            .sort_by(|(id1, _), (id2, _)| Ord::cmp(&id1, &id2));

        assert!(
            new_archetype
                .comp_ids
                .iter()
                .zip(new_archetype.component_storages.iter().map(|(id, _)| id))
                .all(|(id1, id2)| id1 == id2)
        );

        new_archetype.comp_lookup.clear();
        for (n, &id) in new_archetype.comp_ids.iter().enumerate() {
            new_archetype.comp_lookup.insert(id, n);
        }

        new_archetype
    }

    pub fn from_archetype_without(from: &mut Archetype, without_comp_id: EcsId) -> Archetype {
        let mut new_archetype = Archetype::from_archetype(from);

        assert!(new_archetype.comp_lookup.get(&without_comp_id).is_some());

        let remove_idx = new_archetype.comp_lookup[&without_comp_id];
        new_archetype.comp_ids.remove(remove_idx);
        new_archetype.component_storages.remove(remove_idx);

        // TODO there's no need to sort twice they should have the same ordering
        new_archetype.comp_ids.sort();
        new_archetype
            .component_storages
            .sort_by(|(id_1, _), (id_2, _)| Ord::cmp(&id_1, &id_2));

        assert!(
            new_archetype
                .comp_ids
                .iter()
                .zip(new_archetype.component_storages.iter().map(|(id, _)| id))
                .all(|(id1, id2)| id1 == id2)
        );

        new_archetype.comp_lookup.clear();
        for (n, &id) in new_archetype.comp_ids.iter().enumerate() {
            new_archetype.comp_lookup.insert(id, n);
        }

        new_archetype
    }

    pub fn despawn(
        &mut self,
        entity: EcsId,
        entity_idx: usize,
        entity_metas: &mut [Option<EntityMeta>],
    ) -> bool {
        assert!(self.entities[entity_idx] == entity);
        self.entities.swap_remove(entity_idx);
        for storage in self
            .component_storages
            .iter_mut()
            .map(|(_, cell)| cell.get_mut())
        {
            storage.swap_remove(entity_idx);
        }
        entity_metas[entity.uindex()] = None;

        if let Some(&swapped_entity) = self.entities.get(entity_idx) {
            entity_metas[swapped_entity.uindex()]
                .as_mut()
                .unwrap()
                .instance_meta
                .index = entity_idx;
        }

        false
    }

    pub fn try_find_next_archetype(&mut self, id: EcsId) -> Option<usize> {
        self.add_remove_cache.lookup_id(id)
    }

    pub fn insert_archetype_cache(&mut self, id: EcsId, archetype: usize) {
        self.add_remove_cache.insert_id(id, archetype);
    }
}

#[derive(Clone, Debug)]
pub struct EntityMeta {
    /// Metadata for the instance of this EcsId
    pub instance_meta: InstanceMeta,
    /// Metadata for when this EcsId is used as a component
    pub component_meta: ComponentMeta,
}

#[derive(Clone, Debug)]
pub struct ArchIndex(pub usize);
#[derive(Clone, Debug)]
pub struct InstanceMeta {
    pub archetype: ArchIndex,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct ComponentMeta {
    pub drop_fn: Option<fn(*mut core::mem::MaybeUninit<u8>)>,
    pub layout: core::alloc::Layout,
}

fn component_meta_drop_fn<T: Component>(ptr: *mut core::mem::MaybeUninit<u8>) {
    unsafe { core::ptr::drop_in_place::<T>(ptr as *mut T) }
}

impl ComponentMeta {
    pub fn from_size_align(size: usize, align: usize) -> Self {
        Self {
            drop_fn: None,
            layout: core::alloc::Layout::from_size_align(size, align).unwrap(),
        }
    }

    /// Creates a ComponentMeta with the layout and drop_fn of the generic
    pub fn from_generic<T: Component>() -> Self {
        Self {
            drop_fn: Some(component_meta_drop_fn::<T>),
            layout: core::alloc::Layout::new::<T>(),
        }
    }

    /// Creates a unit ComponentMeta, used for when the EcsId should hold no data when added as a component
    pub fn unit() -> Self {
        Self {
            drop_fn: None,
            layout: core::alloc::Layout::new::<()>(),
        }
    }
}

pub struct World {
    pub(crate) archetypes: Vec<Archetype>,
    pub(crate) archetype_bitset: Bitsetsss,
    pub(crate) entities_bitvec: Bitvec,

    entities: Entities,

    ecs_id_meta: Vec<Option<EntityMeta>>,
    pub(crate) type_id_to_ecs_id: HashMap<TypeId, EcsId, crate::utils::TypeIdHasherBuilder>,

    pub(crate) lock_lookup: HashMap<EcsId, usize, crate::utils::TypeIdHasherBuilder>,
    pub(crate) locks: Vec<RwLock<()>>,

    /// usize is that cap allocated with the pointer
    pub(crate) entity_builder_reuse: Option<(Vec<EcsId>, core::ptr::NonNull<u8>, usize)>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some((_, ptr, cap)) = self.entity_builder_reuse.take() {
            unsafe {
                std::alloc::dealloc(
                    ptr.as_ptr(),
                    std::alloc::Layout::from_size_align(cap, 1).unwrap(),
                );
            }
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            archetypes: Vec::new(),
            archetype_bitset: Bitsetsss::with_capacity(32),
            entities_bitvec: Bitvec::with_capacity(32),

            entities: Entities::new(),

            ecs_id_meta: Vec::with_capacity(32),
            type_id_to_ecs_id: HashMap::with_capacity_and_hasher(
                32,
                crate::utils::TypeIdHasherBuilder(),
            ),

            lock_lookup: HashMap::with_hasher(crate::utils::TypeIdHasherBuilder()),
            locks: Vec::new(),

            entity_builder_reuse: None,
        }
    }

    #[must_use]
    /// Creates an entity builder for creating an entity. See the spawn!() macro for a more concise way to use the EntityBuilder
    pub fn spawn(&mut self) -> crate::entity_builder::EntityBuilder {
        let entity = self.entities.spawn();
        crate::entity_builder::EntityBuilder::new(self, entity, ComponentMeta::unit())
    }

    #[must_use]
    /// Same as ``World::spawn`` except takes a capacity to initialise the component storage to
    pub fn spawn_with_capacity(&mut self, capacity: usize) -> crate::entity_builder::EntityBuilder {
        let entity = self.entities.spawn();
        crate::entity_builder::EntityBuilder::with_capacity(
            self,
            entity,
            ComponentMeta::unit(),
            capacity,
        )
    }

    /// Despawns an entity, if the entity being despawned is added as a component to any entities it will be automatically removed
    pub fn despawn(&mut self, entity: EcsId) -> bool {
        if !self.entities.is_alive(entity) {
            return false;
        }

        // TODO: Remove `entity` component from all entities
        for arch in &self.archetypes {
            if let Some(_) = arch.comp_lookup.get(&entity) {
                todo!();
            }
        }

        let InstanceMeta { archetype, index } =
            self.get_entity_meta(entity).unwrap().instance_meta.clone();

        self.archetypes[archetype.0].despawn(entity, index, &mut self.ecs_id_meta);
        self.entities.despawn(entity);
        true
    }

    pub fn is_alive(&mut self, entity: EcsId) -> bool {
        self.entities.is_alive(entity)
    }

    pub fn query_dynamic<const N: usize>(&self, ids: [FetchType; N]) -> DynQuery<'_, N> {
        DynQuery::new(self, ids)
    }

    pub fn query<'a, Q: crate::static_query::QueryTuple>(&'a self) -> StaticQuery<'a, Q> {
        Q::new(self)
    }

    pub fn add_component<T: Component>(&mut self, entity: EcsId, component: T) {
        assert!(self.entities.is_alive(entity));
        let comp_id = self.get_or_create_type_id_ecsid::<T>();
        let mut component = core::mem::ManuallyDrop::new(component);
        unsafe {
            self.add_component_dynamic_with_data(
                entity,
                comp_id,
                &mut component as *mut _ as *mut u8,
            );
        }
    }

    pub fn remove_component<T: Component>(&mut self, entity: EcsId) {
        assert!(self.entities.is_alive(entity));
        let comp_id = self.get_or_create_type_id_ecsid::<T>();
        self.remove_component_dynamic(entity, comp_id);
    }

    pub fn has_component<T: Component>(&self, entity: EcsId) -> bool {
        let func = || {
            let comp_id = self.type_id_to_ecs_id.get(&TypeId::of::<T>())?;
            let ArchIndex(idx) = self.get_entity_meta(entity)?.instance_meta.archetype;
            Some(self.archetypes[idx].comp_lookup.get(comp_id).is_some())
        };

        func().unwrap_or(false)
    }

    pub fn get_component_mut<T: Component>(&mut self, entity: EcsId) -> Option<&mut T> {
        assert!(self.entities.is_alive(entity));
        let comp_id = self.get_or_create_type_id_ecsid::<T>();
        self.get_component_mut_dynamic(entity, comp_id)
            .map(|ptr| unsafe { &mut *{ ptr.cast::<T>() } })
    }

    /// Adds an entity as a dataless component
    ///
    /// This method will panic if a component with the ID of component_id expects data. Entities by default expect no data.
    pub fn add_component_dynamic(&mut self, entity: EcsId, component_id: EcsId) {
        assert!(self.entities.is_alive(entity));
        assert!(self.entities.is_alive(component_id));
        assert!(
            self.get_entity_meta(component_id)
                .unwrap()
                .component_meta
                .layout
                .size()
                == 0
        );

        let mut component = core::mem::ManuallyDrop::new(());
        unsafe {
            self.add_component_dynamic_with_data(
                entity,
                component_id,
                &mut component as *mut _ as *mut u8,
            );
        }
    }
}

impl World {
    /// # Safety
    ///
    ///    All subsequent uses of this entity as a component must be valid for the given ComponentMeta
    #[must_use]
    pub unsafe fn spawn_with_component_meta(
        &mut self,
        component_meta: ComponentMeta,
    ) -> crate::entity_builder::EntityBuilder {
        let entity = self.entities.spawn();

        crate::entity_builder::EntityBuilder::new(self, entity, component_meta)
    }

    pub fn get_or_create_type_id_ecsid<T: Component>(&mut self) -> EcsId {
        let comp_id = self.type_id_to_ecs_id.get(&TypeId::of::<T>());
        if let Some(comp_id) = comp_id {
            return *comp_id;
        }

        let entity = self.spawn().build();

        // Guaranteed valid because we just spawned the entity
        let meta = self.ecs_id_meta[entity.uindex()].as_mut().unwrap();
        meta.component_meta = ComponentMeta::from_generic::<T>();

        self.type_id_to_ecs_id.insert(TypeId::of::<T>(), entity);

        entity
    }

    pub fn get_entity_meta(&self, entity: EcsId) -> Option<&EntityMeta> {
        if !self.entities.is_alive(entity) {
            return None;
        }

        self.ecs_id_meta.get(entity.uindex())?.as_ref()
    }

    pub(crate) fn set_entity_meta(&mut self, entity: EcsId, meta: EntityMeta) {
        if self.entities.is_alive(entity) {
            let new_meta = Some(meta);
            match self.ecs_id_meta.get_mut(entity.uindex()) {
                Some(old_meta) => *old_meta = new_meta,
                None => {
                    self.ecs_id_meta.resize_with(entity.uindex(), || None);
                    self.ecs_id_meta.push(new_meta);
                }
            }
        }
    }

    pub(crate) fn query_archetypes<'a, const N: usize>(
        &'a self,
        iters: [(Iter<'a, usize>, fn(usize) -> usize); N],
        bit_length: u32,
    ) -> ArchetypeIter<'a, N> {
        ArchetypeIter {
            archetypes: &self.archetypes,
            iter: BitsetIterator::new(iters, bit_length),
        }
    }

    pub(crate) fn find_archetype_dynamic(&mut self, comp_ids: &[EcsId]) -> Option<ArchIndex> {
        if self.archetypes.len() > 0 && comp_ids.len() == 0 {
            assert!(self.archetypes[0].comp_ids.len() == 0);
            return Some(ArchIndex(0));
        }

        let mut bit_length = u32::MAX;
        let identity: fn(_) -> _ = |x: usize| x;

        for id in comp_ids {
            let bitvec = self.archetype_bitset.get_bitvec(*id)?;
            if bitvec.len < bit_length as _ {
                bit_length = bitvec.len as _;
            }
        }

        let iters = comp_ids
            .iter()
            .map(|&id| self.archetype_bitset.get_bitvec(id).unwrap().data.iter())
            .map(|bitvec| (bitvec, identity))
            .collect::<Box<[_]>>();

        BitsetIterator::new(iters, bit_length)
            .filter(|idx| self.archetypes[*idx].comp_ids.len() == comp_ids.len())
            .next()
            .map(ArchIndex)
    }

    pub(crate) fn find_archetype_dynamic_plus_id(
        &self,
        comp_ids: &[EcsId],
        extra_id: EcsId,
    ) -> Option<usize> {
        let identity: fn(_) -> _ = |x: usize| x;

        let mut bit_length = u32::MAX;
        for id in comp_ids.iter().chain(std::iter::once(&extra_id)) {
            let bitvec = self.archetype_bitset.get_bitvec(*id)?;
            if bitvec.len < bit_length as _ {
                bit_length = bitvec.len as _;
            }
        }

        let iters = comp_ids
            .iter()
            .map(|&id| self.archetype_bitset.get_bitvec(id).unwrap().data.iter())
            .map(|iter| (iter, identity))
            .chain(std::iter::once((
                self.archetype_bitset
                    .get_bitvec(extra_id)
                    .unwrap()
                    .data
                    .iter(),
                identity,
            )))
            .collect::<Box<[_]>>();

        BitsetIterator::new(iters, bit_length)
            .filter(|idx| self.archetypes[*idx].comp_ids.len() == comp_ids.len() + 1)
            .next()
    }

    pub(crate) fn find_archetype_dynamic_minus_id(
        &self,
        comp_ids: &[EcsId],
        without_id: EcsId,
    ) -> Option<usize> {
        if self.archetypes.len() > 0 && comp_ids.len() == 1 {
            assert!(self.archetypes[0].comp_ids.len() == 0);
            return Some(0);
        }

        let identity: fn(_) -> _ = |x: usize| x;

        let mut bit_length = u32::MAX;
        for id in comp_ids.iter().filter(|&&id| id != without_id) {
            let bitvec = self.archetype_bitset.get_bitvec(*id)?;
            if bitvec.len < bit_length as _ {
                bit_length = bitvec.len as _;
            }
        }

        let iters = comp_ids
            .iter()
            .filter(|&&id| id != without_id)
            .map(|&id| self.archetype_bitset.get_bitvec(id).unwrap().data.iter())
            .map(|iter| (iter, identity))
            .collect::<Box<[_]>>();

        BitsetIterator::new(iters, bit_length)
            .filter(|idx| self.archetypes[*idx].comp_ids.len() == comp_ids.len() - 1)
            .next()
    }

    /// # Safety
    ///
    ///   ``component_ptr`` must point to data that matches the component_meta of component_id.
    ///   The data must also not be used after calling this function.
    pub unsafe fn add_component_dynamic_with_data(
        &mut self,
        entity: EcsId,
        comp_id: EcsId,
        component_ptr: *mut u8,
    ) {
        if !self.entities.is_alive(entity) {
            return;
        }
        if !self.entities.is_alive(comp_id) {
            return;
        }

        let (current_archetype_idx, entity_idx) = {
            let meta = self.get_entity_meta(entity).unwrap();
            (
                meta.instance_meta.archetype.clone(),
                meta.instance_meta.index,
            )
        };
        let current_archetype = &mut self.archetypes[current_archetype_idx.0];
        // Note, this is important, caching will give us *wrong* results if we try and add a component that is in this archetype
        assert!(!current_archetype.comp_ids.contains(&comp_id));

        let target_archetype_idx = current_archetype
            .try_find_next_archetype(comp_id)
            .or_else(|| {
                // Iterate every archeype to see if one exists
                // TODO MAYBE: technically we dont need to iterate everything, we can calculate the exact archetype.type_ids the
                // target archetype will have so we could store a hashmap of that -> archetype_idx in world to avoid this O(n) lookup

                let current_archetype = &self.archetypes[current_archetype_idx.0];
                let idx = self.find_archetype_dynamic_plus_id(&current_archetype.comp_ids, comp_id);

                if let Some(idx) = idx {
                    let current_archetype = &mut self.archetypes[current_archetype_idx.0];
                    current_archetype.insert_archetype_cache(comp_id, idx);
                }

                idx
            })
            .map(ArchIndex)
            .unwrap_or_else(|| {
                // Create a new archetype
                use std::collections::hash_map::Entry;
                let entry = self.lock_lookup.entry(comp_id);
                if let Entry::Vacant(entry) = entry {
                    entry.insert(self.locks.len());
                    self.locks.push(RwLock::new(()));
                }

                let (layout, drop_fn) = {
                    let meta = self
                        .get_entity_meta(comp_id)
                        .unwrap()
                        .component_meta
                        .clone();
                    (meta.layout, meta.drop_fn)
                };

                let archetype = unsafe {
                    Archetype::from_archetype_with(
                        &mut self.archetypes[current_archetype_idx.0],
                        untyped_vec::TypeInfo::new(layout, drop_fn),
                        comp_id,
                    )
                };

                for id in archetype.comp_ids.iter() {
                    self.archetype_bitset
                        .set_bit(*id, self.archetypes.len(), true);
                }
                self.entities_bitvec.push_bit(true);

                self.archetypes.push(archetype);

                let archetypes_len = self.archetypes.len();
                let current_archetype = &mut self.archetypes[current_archetype_idx.0];
                current_archetype.insert_archetype_cache(comp_id, archetypes_len - 1);
                ArchIndex(archetypes_len - 1)
            });

        let (current_archetype, target_archetype) = crate::utils::index_twice_mut(
            current_archetype_idx.0,
            target_archetype_idx.0,
            &mut self.archetypes,
        );

        let mut skipped_idx = None;

        Iterator::zip(
            current_archetype
                .component_storages
                .iter_mut()
                .map(|(_, storage)| storage.get_mut()),
            target_archetype
                .component_storages
                .iter_mut()
                .enumerate()
                // Skip the extra storage in this archetype
                .filter(|(n, (tar_id, _))| {
                    if *tar_id == comp_id {
                        assert!(skipped_idx.is_none());
                        skipped_idx = Some(*n);
                        return false;
                    }
                    true
                })
                .map(|(_, (_, storage))| storage.get_mut()),
        )
        .for_each(|(cur_storage, tar_storage)| unsafe {
            // Safe because component_storages in archetypes are sorted and we skip the component_storage that isn't the same
            cur_storage.swap_move_element_to_other_vec(tar_storage, entity_idx)
        });

        if let None = skipped_idx {
            assert!(*target_archetype.comp_ids.last_mut().unwrap() == comp_id);
            skipped_idx = Some(target_archetype.component_storages.len() - 1);
        }

        unsafe {
            target_archetype.component_storages[skipped_idx.unwrap()]
                .1
                .get_mut()
                .push_raw(component_ptr as *mut core::mem::MaybeUninit<u8>);
        }

        target_archetype.entities.push(entity);
        self.ecs_id_meta[entity.uindex()]
            .as_mut()
            .unwrap()
            .instance_meta = InstanceMeta {
            archetype: target_archetype_idx,
            index: target_archetype.entities.len() - 1,
        };

        current_archetype.entities.swap_remove(entity_idx);
        if let Some(&swapped_entity) = current_archetype.entities.get(entity_idx) {
            self.ecs_id_meta[swapped_entity.uindex()]
                .as_mut()
                .unwrap()
                .instance_meta
                .index = entity_idx;
        }
    }

    pub fn remove_component_dynamic(&mut self, entity: EcsId, comp_id: EcsId) {
        if !self.entities.is_alive(entity) {
            return;
        }
        if !self.entities.is_alive(comp_id) {
            return;
        }

        let (current_archetype_idx, entity_idx) = {
            let meta = self.get_entity_meta(entity).unwrap();
            (
                meta.instance_meta.archetype.clone(),
                meta.instance_meta.index,
            )
        };
        let current_archetype = &mut self.archetypes[current_archetype_idx.0];
        // Note, this is important, caching will give us *wrong* results if we try and remove a component that isnt in this archetype
        assert!(current_archetype.comp_ids.contains(&comp_id));

        let target_archetype_idx = current_archetype
            .try_find_next_archetype(comp_id)
            .or_else(|| {
                // Iterate every archeype to see if one exists
                // TODO MAYBE: technically we dont need to iterate everything, we can calculate the exact archetype.type_ids the
                // target archetype will have so we could store a hashmap of that -> archetype_idx in world to avoid this O(n) lookup

                let current_archetype = &self.archetypes[current_archetype_idx.0];
                let idx =
                    self.find_archetype_dynamic_minus_id(&current_archetype.comp_ids, comp_id);

                if let Some(idx) = idx {
                    let current_archetype = &mut self.archetypes[current_archetype_idx.0];
                    current_archetype.insert_archetype_cache(comp_id, idx);
                }

                idx
            })
            .map(ArchIndex)
            .unwrap_or_else(|| {
                // Create a new archetype
                let archetype = Archetype::from_archetype_without(
                    &mut self.archetypes[current_archetype_idx.0],
                    comp_id,
                );

                for id in archetype.comp_ids.iter() {
                    self.archetype_bitset
                        .set_bit(*id, self.archetypes.len(), true);
                }
                self.entities_bitvec.push_bit(true);

                self.archetypes.push(archetype);

                let archetypes_len = self.archetypes.len();
                let current_archetype = &mut self.archetypes[current_archetype_idx.0];
                current_archetype.insert_archetype_cache(comp_id, archetypes_len - 1);
                ArchIndex(archetypes_len - 1)
            });

        let (current_archetype, target_archetype) = crate::utils::index_twice_mut(
            current_archetype_idx.0,
            target_archetype_idx.0,
            &mut self.archetypes,
        );

        let mut skipped_storage = None;
        Iterator::zip(
            current_archetype
                .component_storages
                .iter_mut()
                .enumerate()
                .filter(|(n, (id, _))| {
                    if *id == comp_id {
                        assert!(skipped_storage.is_none());
                        skipped_storage = Some(*n);
                        return false;
                    }
                    true
                })
                .map(|(_, (_, storage))| storage.get_mut()),
            target_archetype
                .component_storages
                .iter_mut()
                .map(|(_, storage)| storage.get_mut()),
        )
        .for_each(|(cur_storage, tar_storage)| unsafe {
            // Safe because component_storages in archetypes are sorted and we skip the component_storage that isn't the same
            cur_storage.swap_move_element_to_other_vec(tar_storage, entity_idx)
        });

        if skipped_storage.is_none() {
            assert!(*current_archetype.comp_ids.last_mut().unwrap() == comp_id);
            skipped_storage = Some(current_archetype.component_storages.len() - 1);
        }

        current_archetype.component_storages[skipped_storage.unwrap()]
            .1
            .get_mut()
            .swap_remove(entity_idx);

        target_archetype.entities.push(entity);
        self.ecs_id_meta[entity.uindex()]
            .as_mut()
            .unwrap()
            .instance_meta = InstanceMeta {
            archetype: target_archetype_idx,
            index: target_archetype.entities.len() - 1,
        };

        current_archetype.entities.swap_remove(entity_idx);
        if let Some(&swapped_entity) = current_archetype.entities.get(entity_idx) {
            self.ecs_id_meta[swapped_entity.uindex()]
                .as_mut()
                .unwrap()
                .instance_meta
                .index = entity_idx;
        }
    }

    pub fn get_component_mut_dynamic(&mut self, entity: EcsId, comp_id: EcsId) -> Option<*mut u8> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        if !self.entities.is_alive(comp_id) {
            return None;
        }

        let (archetype_idx, entity_idx) = {
            let meta = self.get_entity_meta(entity)?;
            (
                meta.instance_meta.archetype.clone(),
                meta.instance_meta.index,
            )
        };
        let archetype = &mut self.archetypes[archetype_idx.0];

        let component_storage_idx = archetype.comp_lookup[&comp_id];

        Some(
            archetype.component_storages[component_storage_idx]
                .1
                .get_mut()
                .get_mut_raw(entity_idx)
                .unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn;

    #[test]
    pub fn spawn() -> () {
        let mut world = World::new();
        let _ = world.spawn();
    }

    #[test]
    pub fn get() {
        let mut world = World::new();

        let entity = spawn!(&mut world, 10_u32, 12_u64, "Hello");
        let entity2 = spawn!(&mut world, 18_u32, "AWDAWDAWD", 16.0f32);

        let str_comp: &mut &str = world.get_component_mut(entity).unwrap();
        assert!(*str_comp == "Hello");

        let str_comp: &mut &str = world.get_component_mut(entity2).unwrap();
        assert!(*str_comp == "AWDAWDAWD");
    }

    #[test]
    pub fn entity_archetype_lookup() {
        let mut world = World::new();

        let entity = spawn!(&mut world, 10_u32, 12_u64);

        let entity_meta = world.ecs_id_meta[entity.uindex()].clone().unwrap();
        assert!(entity_meta.instance_meta.index == 0);
        assert!(entity_meta.instance_meta.archetype.0 == 1);
    }

    #[test]
    pub fn add_component() {
        let mut world = World::new();
        let entity = spawn!(&mut world, 1_u32);
        world.add_component(entity, 2_u64);

        assert!(world.archetypes.len() == 3);
        let entity_meta = world.ecs_id_meta[entity.uindex()].clone().unwrap();
        assert!(entity_meta.instance_meta.archetype.0 == 2);
        assert!(entity_meta.instance_meta.index == 0);

        // The two component entities
        assert!(world.archetypes[0].entities.len() == 2);
        assert!(world.archetypes[0].component_storages.len() == 0);
        for (_, lock) in world.archetypes[0].component_storages.iter_mut() {
            let storage = lock.get_mut();
            assert!(storage.len() == 0);
        }

        // The first archetype entity was in
        assert!(world.archetypes[1].entities.len() == 0);
        assert!(world.archetypes[1].component_storages.len() == 1);
        for (_, lock) in world.archetypes[1].component_storages.iter_mut() {
            let storage = lock.get_mut();
            assert!(storage.len() == 0);
        }

        // The current archetype entity was in
        assert!(world.archetypes[2].entities.len() == 1);
        assert!(world.archetypes[2].component_storages.len() == 2);
        for (_, lock) in world.archetypes[2].component_storages.iter_mut() {
            let storage = lock.get_mut();
            assert!(storage.len() == 1);
        }

        let mut run_times = 0;
        let mut query = world.query::<(&u32, &u64)>();
        query.iter().for_each(|(left, right)| {
            assert!(*left == 1);
            assert!(*right == 2);
            run_times += 1;
        });
        assert!(run_times == 1);
    }

    #[test]
    pub fn add_component_then_spawn() {
        let mut world = World::new();
        let entity = spawn!(&mut world, 1_u32);
        world.add_component(entity, 2_u64);

        let entity2 = spawn!(&mut world, 3_u32, 4_u64);

        assert!(world.archetypes.len() == 3);

        // Component entities
        assert!(world.archetypes[0].entities.len() == 2);
        assert!(world.archetypes[0].component_storages.len() == 0);

        // Original first entity archetype
        assert!(world.archetypes[1].entities.len() == 0);
        assert!(world.archetypes[1].component_storages.len() == 1);
        assert!(world.archetypes[1].component_storages[0].1.get_mut().len() == 0);

        // Entity2 + Entity1 Archetpye
        assert!(world.archetypes[2].entities.len() == 2);
        assert!(world.archetypes[2].entities[0] == entity);
        assert!(world.archetypes[2].entities[1] == entity2);
        assert!(world.archetypes[2].component_storages.len() == 2);
        assert!(world.archetypes[2].component_storages[0].1.get_mut().len() == 2);
        assert!(world.archetypes[2].component_storages[1].1.get_mut().len() == 2);

        let entity_meta = world.ecs_id_meta[entity.uindex()].clone().unwrap();
        assert!(entity_meta.instance_meta.archetype.0 == 2);
        assert!(entity_meta.instance_meta.index == 0);

        let entity_meta = world.ecs_id_meta[entity2.uindex()].clone().unwrap();
        assert!(entity_meta.instance_meta.archetype.0 == 2);
        assert!(entity_meta.instance_meta.index == 1);

        let mut run_times = 0;
        let mut checks = vec![(1, 2), (3, 4)].into_iter();
        let mut query = world.query::<(&u32, &u64)>();
        query.iter().for_each(|(left, right)| {
            assert!(checks.next().unwrap() == (*left, *right));
            run_times += 1;
        });
        assert!(run_times == 2);
    }

    #[test]
    pub fn add_two() {
        struct A(f32);
        struct B(f32);

        let mut world = World::new();
        let entity_1 = spawn!(&mut world, A(1.));
        let entity_2 = spawn!(&mut world, A(1.));

        assert!(world.archetypes[0].entities.len() == 1);
        assert!(world.archetypes[0].component_storages.len() == 0);

        let entity_1_meta = world.ecs_id_meta[entity_1.uindex()].clone().unwrap();
        assert!(world.archetypes[1].entities[0] == entity_1);
        assert!(entity_1_meta.instance_meta.archetype.0 == 1);
        assert!(entity_1_meta.instance_meta.index == 0);

        let entity_2_meta = world.ecs_id_meta[entity_2.uindex()].clone().unwrap();
        assert!(world.archetypes[1].entities[1] == entity_2);
        assert!(entity_2_meta.instance_meta.archetype.0 == 1);
        assert!(entity_2_meta.instance_meta.index == 1);

        world.add_component(entity_1, B(2.));
        assert!(world.archetypes[0].entities.len() == 2);

        assert!(world.archetypes[1].entities[0] == entity_2);
        assert!(world.archetypes[1].entities.len() == 1);
        assert!(world.archetypes[2].entities[0] == entity_1);
        assert!(world.archetypes[2].entities.len() == 1);

        let entity_1_meta = world.ecs_id_meta[entity_1.uindex()].clone().unwrap();
        assert!(entity_1_meta.instance_meta.archetype.0 == 2);
        assert!(entity_1_meta.instance_meta.index == 0);

        let entity_2_meta = world.ecs_id_meta[entity_2.uindex()].clone().unwrap();
        assert!(entity_2_meta.instance_meta.archetype.0 == 1);
        assert!(entity_2_meta.instance_meta.index == 0);

        world.add_component(entity_2, B(2.));
        assert!(world.archetypes[0].entities.len() == 2);
        assert!(world.archetypes[1].entities.len() == 0);
        assert!(world.archetypes[2].entities.len() == 2);

        assert!(world.archetypes[2].entities[0] == entity_1);
        assert!(world.archetypes[2].entities[1] == entity_2);

        let entity_1_meta = world.ecs_id_meta[entity_1.uindex()].clone().unwrap();
        assert!(entity_1_meta.instance_meta.archetype.0 == 2);
        assert!(entity_1_meta.instance_meta.index == 0);

        let entity_2_meta = world.ecs_id_meta[entity_2.uindex()].clone().unwrap();
        assert!(entity_2_meta.instance_meta.archetype.0 == 2);
        assert!(entity_2_meta.instance_meta.index == 1);
    }

    #[test]
    pub fn add_multiple() {
        struct A(f32);
        struct B(f32);

        let mut world = World::new();
        let mut entities = Vec::with_capacity(500);

        for _ in 0..10 {
            entities.push(spawn!(&mut world, A(1.)));
        }

        for &entity in entities.iter() {
            world.add_component(entity, B(1.));
        }
        for &entity in entities.iter() {
            world.remove_component::<B>(entity);
        }
    }

    #[test]
    pub fn despawn_meta_update() {
        let mut world = World::new();

        let e1 = world.spawn().with(10_u32).build();
        let e2 = world.spawn().with(12_u32).build();
        let e3 = world.spawn().with(14_u32).build();

        assert!(world.despawn(e1));

        assert!(world.is_alive(e1) == false);
        assert!(world.ecs_id_meta[e1.uindex()].is_none());

        assert!(world.is_alive(e2));
        assert!(world.is_alive(e3));

        assert!(*world.get_component_mut::<u32>(e2).unwrap() == 12);
        assert!(*world.get_component_mut::<u32>(e3).unwrap() == 14);
    }

    #[test]
    pub fn despawn_component_entity() {
        // TODO: Removing entities when they despawn not yet implemented
        return;
        let mut world = World::new();

        unsafe {
            let component_entity = world
                .spawn_with_component_meta(ComponentMeta::from_generic::<u32>())
                .build();

            let e1 = world
                .spawn()
                .with_dynamic_with_data(&mut 10_u32 as *mut _ as *mut _, component_entity)
                .build();
            let e2 = world
                .spawn()
                .with_dynamic_with_data(&mut 10_u32 as *mut _ as *mut _, component_entity)
                .build();
            let e3 = world
                .spawn()
                .with_dynamic_with_data(&mut 10_u32 as *mut _ as *mut _, component_entity)
                .build();

            world.despawn(component_entity);

            assert!(world.archetypes.len() == 2);

            let assert_meta = |world: &mut World, entity: EcsId, archetype_idx, entity_idx| {
                let meta = world.ecs_id_meta[entity.uindex()].as_ref().unwrap();
                assert!(meta.instance_meta.archetype.0 == archetype_idx);
                assert!(meta.instance_meta.index == entity_idx);
            };

            assert!(world.archetypes[0].entities == &[e1, e2, e3]);
            assert_meta(&mut world, e1, 0, 0);
            assert_meta(&mut world, e2, 0, 1);
            assert_meta(&mut world, e3, 0, 2);

            assert!(world.archetypes[1].entities.len() == 0);
            assert!(world.ecs_id_meta[component_entity.uindex()].is_none());
        }
    }

    // TODO: Boxy can you make the following tests actually work?
    // Currently they basically just want to not panic, but they should check capacity if possible
    #[test]
    pub fn spawn_with_capacity() -> () {
        let mut world = World::new();
        let entity = world.spawn_with_capacity(32).build();
        assert_eq!(entity, EcsId::new(0, 0));
    }

    #[test]
    pub fn spawn_with_capacity_zero() -> () {
        let mut world = World::new();
        let entity = world.spawn_with_capacity(0).build();
        assert_eq!(entity, EcsId::new(0, 0));
    }

    #[test]
    pub fn test_alive() -> () {
        let mut world = World::new();
        let alive = world.spawn().build();
        let dead = world.spawn().build();

        world.despawn(dead);

        assert!(world.is_alive(alive));
        assert!(!world.is_alive(dead));
    }
}

// ComponentMeta::from_size_align can panic when the requirements for Layout::from_size_align are not met
#[no_mangle]
pub extern "C" fn _component_meta_from_size_align(size: usize, align: usize) -> Box<ComponentMeta> {
    match catch_unwind(|| Box::new(ComponentMeta::from_size_align(size, align))) {
        Ok(meta) => meta,
        Err(_) => abort(),
    }
}

// Cannot panic, no need for catch_unwind
#[no_mangle]
pub extern "C" fn _component_meta_unit() -> Box<ComponentMeta> {
    Box::new(ComponentMeta::unit())
}

// Cannot panic, no need for catch_unwind
#[no_mangle]
pub extern "C" fn _world_new() -> Box<World> {
    Box::new(World::new())
}

// Cannot panic, no need for catch_unwind
/// # Safety
///
/// * `_world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub unsafe extern "C" fn _world_drop(_world: Option<Box<World>>) -> () {}

// Possible, but unlikely, to panic in Entities::spawn or EntityBuilder::new through World::spawn
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub unsafe extern "C" fn _world_spawn(world: &mut World) -> *mut u8 {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(world.spawn())) as *mut u8
    })) {
        Ok(entity) => entity,
        Err(_) => abort(),
    }
}

// Possible, but unlikely, to panic in Entities::spawn or EntityBuilder::new through World::spawn_with_component_meta
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
/// * `component_meta` must be a valid pointer to a `ComponentMeta`
#[no_mangle]
pub unsafe extern "C" fn _world_spawn_with_component_meta(
    world: &mut World,
    component_meta: &mut ComponentMeta,
) -> *mut u8 {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        let builder = world.spawn_with_component_meta(component_meta.clone());
        Box::into_raw(Box::new(builder)) as *mut u8
    })) {
        Ok(builder) => builder,
        Err(_) => abort(),
    }
}

// Could panic in lots of ways
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub extern "C" fn _world_despawn(world: &mut World, entity: EcsId) -> bool {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| world.despawn(entity))) {
        Ok(success) => success,
        Err(_) => abort(),
    }
}

// TODO: this really shouldn't panic, but it can
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub extern "C" fn _world_is_alive(world: &mut World, entity: EcsId) -> bool {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| world.is_alive(entity))) {
        Ok(success) => success,
        Err(_) => abort(),
    }
}

// probably panics a lot
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub unsafe extern "C" fn _world_add_component_dynamic(
    world: &mut World,
    entity: EcsId,
    component_id: EcsId,
) -> () {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| {
        world.add_component_dynamic(entity, component_id)
    })) {
        Ok(_) => {}
        Err(_) => abort(),
    }
}

// probably panics a lot
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
/// * `component_ptr` must be a valid pointer to data that matches the component meta on the entity `comp_id`
#[no_mangle]
pub unsafe extern "C" fn _world_add_component_dynamic_with_data(
    world: &mut World,
    entity: EcsId,
    comp_id: EcsId,
    component_ptr: *mut u8,
) -> () {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        world.add_component_dynamic_with_data(entity, comp_id, component_ptr);
    })) {
        Ok(_) => {}
        Err(_) => abort(),
    }
}

// probably panics a lot
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub unsafe extern "C" fn _world_remove_component_dynamic(
    world: &mut World,
    entity: EcsId,
    comp_id: EcsId,
) -> () {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| {
        world.remove_component_dynamic(entity, comp_id);
    })) {
        Ok(_) => {}
        Err(_) => abort(),
    }
}

// probably panics a lot
/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
#[no_mangle]
pub unsafe extern "C" fn _world_get_component_mut_dynamic(
    world: &mut World,
    entity: EcsId,
    comp_id: EcsId,
) -> *mut u8 {
    // AssertUnwindSafe used because World is not unwind safe
    // However, since the panic case is handled with an abort, it is fine
    match catch_unwind(AssertUnwindSafe(|| {
        world
            .get_component_mut_dynamic(entity, comp_id)
            .unwrap_or(ptr::null_mut())
    })) {
        Ok(component) => component,
        Err(_) => abort(),
    }
}
