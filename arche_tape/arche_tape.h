#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

template<typename T = void>
struct Box;

struct ComponentMeta;

template<typename T = void>
struct Option;

struct World;

using EcsIdGen = uint32_t;

using EcsIdIndex = uint32_t;

struct EcsId {
  EcsIdGen _0;
  EcsIdIndex _1;
};

struct FFIDynQuery {
  uint8_t *ptr;
  uintptr_t len;
};

union FetchType {
  enum class Tag : uint8_t {
    EcsId,
    Mut,
    Immut,
  };

  struct Mut_Body {
    Tag tag;
    EcsId _0;
  };

  struct Immut_Body {
    Tag tag;
    EcsId _0;
  };

  struct {
    Tag tag;
  };
  Mut_Body mut;
  Immut_Body immut;
};

struct FFIDynQueryIter {
  uint8_t *ptr;
  uintptr_t len;
};

struct FFIDynQueryResult {
  uint8_t *ptr;
  uintptr_t len;
};

extern "C" {

/// # Safety
///
/// * `builder` must be a valid pointer to an `EntityBuilder` created by one of the spawn methods on World
EcsId _entitybuilder_build(uint8_t *builder);

/// # Safety
///
/// * `builder` must be a valid pointer to an `EntityBuilder` created by one of the spawn methods on World
/// * `component` must be a valid pointer to a component that matches the component meta on `component_id`
uint8_t *_entitybuilder_with_dynamic(uint8_t *builder,
                                     uint8_t *component,
                                     EcsId component_id);

Box<ComponentMeta> _component_meta_from_size_align(uintptr_t size, uintptr_t align);

Box<ComponentMeta> _component_meta_unit();

Box<World> _world_new();

/// # Safety
///
/// * `_world` must be a valid pointer to a `World` created by `_world_new()`
void _world_drop(Option<Box<World>> _world);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
uint8_t *_world_spawn(World *world);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
/// * `component_meta` must be a valid pointer to a `ComponentMeta`
uint8_t *_world_spawn_with_component_meta(World *world, ComponentMeta *component_meta);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
bool _world_despawn(World *world, EcsId entity);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
bool _world_is_alive(World *world, EcsId entity);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
void _world_add_component_dynamic(World *world, EcsId entity, EcsId component_id);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
/// * `component_ptr` must be a valid pointer to data that matches the component meta on the entity `comp_id`
void _world_add_component_dynamic_with_data(World *world,
                                            EcsId entity,
                                            EcsId comp_id,
                                            uint8_t *component_ptr);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
void _world_remove_component_dynamic(World *world, EcsId entity, EcsId comp_id);

/// # Safety
///
/// * `world` must be a valid pointer to a `World` created by `_world_new()`
uint8_t *_world_get_component_mut_dynamic(World *world, EcsId entity, EcsId comp_id);

FFIDynQuery _dyn_query_new(const World *world, const FetchType *fetches, uintptr_t len);

FFIDynQueryIter _dyn_query_iter(FFIDynQuery q);

FFIDynQueryResult _dyn_query_next(FFIDynQueryIter qi);

} // extern "C"
