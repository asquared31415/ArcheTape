#[cfg(test)]
mod ffi_tests {
    mod world {
        use crate::world;

        #[test]
        fn world_new() -> () {
            let _ptr = world::_world_new();
        }

        #[test]
        fn world_drop() -> () {
            let ptr = world::_world_new();

            unsafe {
                world::_world_drop(Some(ptr));
            };
        }
    }

    mod entity_builder {
        use crate::{entity_builder, world, EcsId};

        #[test]
        fn entity_builder_build() -> () {
            let mut world = world::_world_new();
            let builder = unsafe { world::_world_spawn(world.as_mut()) };

            assert!(!builder.is_null());

            let entity = unsafe { entity_builder::_entitybuilder_build(builder) };
            assert_eq!(entity, EcsId::new(0, 0));
        }
    }

    mod world_entity {
        use crate::{entity_builder, world, EcsId};

        #[test]
        fn world_spawn() -> () {
            let mut world = world::_world_new();
            let builder = unsafe { world::_world_spawn(world.as_mut()) };

            assert!(!builder.is_null());
        }

        #[test]
        fn world_is_alive() -> () {
            let mut world = world::_world_new();
            let alive_builder = unsafe { world::_world_spawn(world.as_mut()) };
            assert!(!alive_builder.is_null());
            let alive = unsafe { entity_builder::_entitybuilder_build(alive_builder) };
            assert_eq!(alive, EcsId::new(0, 0));

            let dead_builder = unsafe { world::_world_spawn(world.as_mut()) };
            assert!(!dead_builder.is_null());
            let dead = unsafe { entity_builder::_entitybuilder_build(dead_builder) };
            assert_eq!(dead, EcsId::new(1, 0));

            world::_world_despawn(world.as_mut(), dead);

            assert!(world::_world_is_alive(world.as_mut(), alive));
            assert!(!world::_world_is_alive(world.as_mut(), dead));
        }

        #[test]
        fn world_despawn() -> () {
            let mut world = world::_world_new();
            let builder = unsafe { world::_world_spawn(world.as_mut()) };

            assert!(!builder.is_null());

            let entity = unsafe { entity_builder::_entitybuilder_build(builder) };
            assert_eq!(entity, EcsId::new(0, 0));

            world::_world_despawn(world.as_mut(), entity);
            assert!(!world::_world_is_alive(world.as_mut(), entity));
        }
    }

    mod dyn_query {
        use crate::{
            dyn_query::*,
            entity_builder::*,
            world::*,
        };
        use core::slice;

        #[test]
        fn dyn_query() {
            let mut world = _world_new();

            let u32_id = unsafe {
                _entitybuilder_build(_world_spawn_with_component_meta(
                    &mut world,
                    &mut _component_meta_from_size_align(4, 4),
                ))
            };

            let i32_id = unsafe {
                _entitybuilder_build(_world_spawn_with_component_meta(
                    &mut world,
                    &mut _component_meta_from_size_align(4, 4),
                ))
            };

            let _entity = unsafe {
                _entitybuilder_build(_entitybuilder_with_dynamic(
                    _entitybuilder_with_dynamic(
                        _world_spawn(&mut world),
                        &mut 1u32 as *mut u32 as *mut u8,
                        u32_id,
                    ),
                    &mut -2 as *mut i32 as *mut u8,
                    i32_id,
                ))
            };

            let _entity2 = unsafe {
                _entitybuilder_build(_entitybuilder_with_dynamic(
                    _world_spawn(&mut world),
                    &mut 2u32 as *mut u32 as *mut u8,
                    u32_id,
                ))
            };

            let _entity3 = unsafe {
                _entitybuilder_build(_entitybuilder_with_dynamic(
                    _entitybuilder_with_dynamic(
                        _world_spawn(&mut world),
                        &mut 999u32 as *mut u32 as *mut u8,
                        u32_id,
                    ),
                    &mut -2048 as *mut i32 as *mut u8,
                    i32_id,
                ))
            };

            let fetches = [FetchType::Immut(u32_id), FetchType::Immut(i32_id)];
            unsafe {
                let query = _dyn_query_new(&mut world, fetches.as_ptr(), fetches.len());
                let iter = _dyn_query_iter(query);
                let FFIDynQueryResult { ptr, len } = _dyn_query_next(iter);
                let data = slice::from_raw_parts(ptr as *mut PtrLen, len);

                let mut collected = [(0u32, 0i32); 2];
                for i in 0..data[0].1 {
                    collected[i].0 = *(data[0].0.offset((4 * i) as isize) as *mut u32);
                }

                for i in 0..data[1].1 {
                    collected[i].1 = *(data[1].0.offset((4 * i) as isize) as *mut i32);
                }

                assert_eq!(collected, [(1, -2), (999, -2048)]);
            }
        }
    }
}
