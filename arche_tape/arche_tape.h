#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

struct World;

using EcsIdGen = uint32_t;

using EcsIdIndex = uint32_t;

struct EcsId {
  EcsIdGen _0;
  EcsIdIndex _1;
};

extern "C" {

uint8_t *_component_meta_from_size_align(uintptr_t size, uintptr_t align);

uint8_t *_component_meta_unit();

World *_world_new();

void _world_drop(World *world);

uint8_t *_world_spawn(World *world);

uintptr_t _world_despawn(World *world, EcsId entity);

uint8_t _world_is_alive(World *world, EcsId entity);

void _world_add_component_dynamic(World *world, EcsId entity, EcsId component_id);

uint8_t *_world_spawn_with_component_meta(World *world, uint8_t *component_meta);

void _world_add_component_dynamic_with_data(World *world,
                                            EcsId entity,
                                            EcsId comp_id,
                                            uint8_t *component_ptr);

void _world_remove_component_dynamic(World *world, EcsId entity, EcsId comp_id);

uint8_t *_world_get_component_mut_dynamic(World *world, EcsId entity, EcsId comp_id);

} // extern "C"
