using System;
using System.Runtime.InteropServices;

namespace ArcheTapeFFI
{
    public class Name
    {
#if Windows
        public const string LibName = "arche_tape.dll";
#elif Linux
        public const string LibName = "TODO";
#endif
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct EcsId
    {
        public uint gen;
        public uint index;

        public override string ToString()
        {
            return $"Generation: {gen:X8}, Index: {index:X8}";
        }
    }

    public class World
    {
        private readonly IntPtr ptr;

        public World()
        {
            ptr = _world_new();
        }

        ~World()
        {
            _world_drop(ptr);
        }

        public EntityBuilder Spawn()
        {
            var builder = _world_spawn(ptr);
            return new EntityBuilder(builder);
        }

        public EntityBuilder SpawnWithComponentMeta(ComponentMeta meta)
        {
            var builder = _world_spawn_with_component_meta(ptr, meta.Ptr);
            return new EntityBuilder(builder);
        }

        public void Despawn(EcsId entity)
        {
            _world_despawn(ptr, entity);
        }

        public bool IsAlive(EcsId entity)
        {
            return _world_is_alive(ptr, entity);
        }

        public void AddComponentDynamic(EcsId entity, EcsId componentId)
        {
            _world_add_component_dynamic(ptr, entity, componentId);
        }

        public unsafe void AddComponentDynamicWithData(EcsId entity, EcsId componentId, void* data)
        {
            _world_add_component_dynamic_with_data(ptr, entity, componentId, data);
        }

        public void RemoveComponentDynamic(EcsId entity, EcsId componentId)
        {
            _world_remove_component_dynamic(ptr, entity, componentId);
        }

        public IntPtr GetComponentDynamic(EcsId entity, EcsId componentId)
        {
            return _world_get_component_mut_dynamic(ptr, entity, componentId);
        }

        [DllImport(Name.LibName)]
        private static extern IntPtr _world_new();

        [DllImport(Name.LibName)]
        private static extern void _world_drop(IntPtr world);

        [DllImport(Name.LibName)]
        private static extern IntPtr _world_spawn(IntPtr world);

        [DllImport(Name.LibName)]
        private static extern IntPtr _world_spawn_with_component_meta(IntPtr world, IntPtr meta);

        [DllImport(Name.LibName)]
        private static extern IntPtr _world_despawn(IntPtr world, EcsId entity);

        [DllImport(Name.LibName)]
        private static extern bool _world_is_alive(IntPtr world, EcsId entity);

        [DllImport(Name.LibName)]
        private static extern void _world_add_component_dynamic(IntPtr world, EcsId entity, EcsId componentId);

        [DllImport(Name.LibName)]
        private static extern unsafe void _world_add_component_dynamic_with_data(
            IntPtr world,
            EcsId entity,
            EcsId componentId,
            void* componentPtr
        );

        [DllImport(Name.LibName)]
        private static extern void _world_remove_component_dynamic(IntPtr world, EcsId entity, EcsId componentId);

        [DllImport(Name.LibName)]
        private static extern IntPtr _world_get_component_mut_dynamic(IntPtr world, EcsId entity, EcsId componentId);
    }

    public class EntityBuilder
    {
        private readonly IntPtr ptr;

        internal EntityBuilder(IntPtr ptr)
        {
            this.ptr = ptr;
        }

        public EcsId Build()
        {
            return _entitybuilder_build(ptr);
        }

        [DllImport(Name.LibName)]
        private static extern EcsId _entitybuilder_build(IntPtr builder);
    }

    public class ComponentMeta
    {
        internal readonly IntPtr Ptr;

        private ComponentMeta(IntPtr ptr)
        {
            this.Ptr = ptr;
        }

        public static ComponentMeta FromSizeAlign(nuint size, nuint align)
        {
            return new ComponentMeta(_component_meta_from_size_align(size, align));
        }

        public static ComponentMeta Unit()
        {
            return new ComponentMeta(_component_meta_unit());
        }

        [DllImport(Name.LibName)]
        private static extern IntPtr _component_meta_from_size_align(nuint size, nuint align);

        [DllImport(Name.LibName)]
        private static extern IntPtr _component_meta_unit();
    }

    public static class Utils
    {
        public static IntPtr Allocate<T>(T data)
        {
            var ptr = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(T)));
            Marshal.StructureToPtr(data, ptr, false);
            return ptr;
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct StructData
    {
        public byte f1;
        public byte f2;
        public int f3;

        public override string ToString()
        {
            return $"(f1: {f1}, f2: {f2}, f3: {f3})";
        }
    }

    public class Program
    {
        public static unsafe void Main(string[] args)
        {
            var world = new World();
            var entity = world.Spawn().Build();
            Console.WriteLine($"entity: {entity}");

            var meta = ComponentMeta.FromSizeAlign(8, 4);
            var dataId = world.SpawnWithComponentMeta(meta).Build();
            Console.WriteLine($"data entity: {dataId}");

            var data = new StructData { f1 = 42, f2 = 10, f3 = 1337};
            world.AddComponentDynamicWithData(entity, dataId, &data);

            var getData = world.GetComponentDynamic(entity, dataId);
            Console.WriteLine($"got data at: 0x{getData:X16}: {*(StructData*) getData.ToPointer()}");
        }
    }
}