#[cfg(test)]
mod ffi_tests {
    mod world {
        use crate::world;

        #[test]
        fn world_new() -> () {
            let ptr = world::_world_new();
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
}
