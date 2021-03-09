using System;
using System.Collections;
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
        public IntPtr Ptr { get; }

        public World()
        {
            Ptr = _world_new();
        }

        ~World()
        {
            _world_drop(Ptr);
        }

        public EntityBuilder Spawn()
        {
            var builder = _world_spawn(Ptr);
            return new EntityBuilder(builder);
        }

        public EntityBuilder SpawnWithComponentMeta(ComponentMeta meta)
        {
            var builder = _world_spawn_with_component_meta(Ptr, meta.Ptr);
            return new EntityBuilder(builder);
        }

        public void Despawn(EcsId entity)
        {
            _world_despawn(Ptr, entity);
        }

        public bool IsAlive(EcsId entity)
        {
            return _world_is_alive(Ptr, entity);
        }

        public void AddComponentDynamic(EcsId entity, EcsId componentId)
        {
            _world_add_component_dynamic(Ptr, entity, componentId);
        }

        public unsafe void AddComponentDynamicWithData(EcsId entity, EcsId componentId, void* data)
        {
            _world_add_component_dynamic_with_data(Ptr, entity, componentId, data);
        }

        public void RemoveComponentDynamic(EcsId entity, EcsId componentId)
        {
            _world_remove_component_dynamic(Ptr, entity, componentId);
        }

        public IntPtr GetComponentDynamic(EcsId entity, EcsId componentId)
        {
            return _world_get_component_mut_dynamic(Ptr, entity, componentId);
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
        private IntPtr ptr;

        internal EntityBuilder(IntPtr ptr)
        {
            this.ptr = ptr;
        }

        public EcsId Build()
        {
            return _entitybuilder_build(ptr);
        }

        public EntityBuilder WithDynamic(IntPtr component, EcsId componentId)
        {
            ptr = _entitybuilder_with_dynamic(ptr, component, componentId);
            return this;
        }

        [DllImport(Name.LibName)]
        private static extern EcsId _entitybuilder_build(IntPtr builder);

        [DllImport(Name.LibName)]
        private static extern IntPtr _entitybuilder_with_dynamic(IntPtr builder, IntPtr component, EcsId componentId);
    }

    public class ComponentMeta
    {
        internal readonly IntPtr Ptr;

        private ComponentMeta(IntPtr ptr)
        {
            Ptr = ptr;
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

    public class DynQuery : IEnumerable
    {
        private FFIDynQuery query;
        private FFIDynQueryIter? iterator;

        public unsafe DynQuery(World world, FetchType[] fetches)
        {
            fixed (FetchType* f = fetches)
            {
                query = _dyn_query_new(world.Ptr, f, (nuint) fetches.Length);
            }
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct FFIDynQuery
        {
            private IntPtr ptr;
            private nuint len;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct FFIDynQueryIter
        {
            private IntPtr ptr;
            private nuint len;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct FFIDynQueryResult
        {
            internal IntPtr ptr;
            internal nuint len;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct PtrLen
        {
            internal IntPtr ptr;
            internal nuint len;
        }

        public IEnumerator GetEnumerator()
        {
            iterator ??= _dyn_query_iter(query);

            while (true)
            {
                var next = _dyn_query_next(iterator.Value);
                if (next.len == 0)
                {
                    break;
                }

                yield return Utils.MarshalToArray<PtrLen>(next.ptr, (int) next.len);
            }
        }

        [DllImport(Name.LibName)]
        private static extern unsafe FFIDynQuery _dyn_query_new(IntPtr world, FetchType* fetches, nuint len);

        [DllImport(Name.LibName)]
        private static extern FFIDynQueryIter _dyn_query_iter(FFIDynQuery query);

        [DllImport(Name.LibName)]
        private static extern FFIDynQueryResult _dyn_query_next(FFIDynQueryIter query);
    }


    public static class Utils
    {
        public static IntPtr Allocate<T>(T data)
        {
            var ptr = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(T)));
            Marshal.StructureToPtr(data, ptr, false);
            return ptr;
        }

        public static T[] MarshalToArray<T>(IntPtr unmanagedArray, int length)
        {
            var size = Marshal.SizeOf(typeof(T));
            var managedArray = new T[length];

            for (var i = 0; i < length; i++)
            {
                var ins = new IntPtr(unmanagedArray.ToInt64() + i * size);
                managedArray[i] = Marshal.PtrToStructure<T>(ins);
            }

            return managedArray;
        }
    }

    public enum FetchTypeTag : byte
    {
        EcsId,
        Mut,
        Immut,
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MutFetch
    {
        private FetchTypeTag tag;
        private EcsId id;

        public MutFetch(EcsId id)
        {
            tag = FetchTypeTag.Mut;
            this.id = id;
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct ImmutFetch
    {
        private FetchTypeTag tag;
        private EcsId id;

        public ImmutFetch(EcsId id)
        {
            tag = FetchTypeTag.Immut;
            this.id = id;
        }
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct FetchType
    {
        [FieldOffset(0)] public FetchTypeTag tag;
        [FieldOffset(0)] public MutFetch mutFetch;
        [FieldOffset(0)] public ImmutFetch immutFetch;
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

            var dataId = world.SpawnWithComponentMeta(ComponentMeta.FromSizeAlign(8, 4)).Build();
            var intId = world.SpawnWithComponentMeta(ComponentMeta.FromSizeAlign(4, 4)).Build();

            var data = new StructData {f1 = 42, f2 = 10, f3 = 1337};
            world.Spawn().WithDynamic((IntPtr) (&data), dataId).WithDynamic(Utils.Allocate(111), intId).Build();
            data = new StructData {f1 = 42, f2 = 10, f3 = 99999};
            world.Spawn().WithDynamic((IntPtr) (&data), dataId).Build();

            var fetches = new[]
                          {
                              new FetchType
                              {
                                  immutFetch = new ImmutFetch(dataId)
                              },
                              new FetchType
                              {
                                  immutFetch = new ImmutFetch(intId)
                              }
                          };

            var query = new DynQuery(world, fetches);
            foreach (DynQuery.PtrLen[] col in query)
            {
                for (var i = 0; i < (int) col[0].len; ++i)
                {
                    Console.WriteLine(*(StructData*) (col[0].ptr + i * Marshal.SizeOf<StructData>()));
                }

                for (var i = 0; i < (int) col[1].len; ++i)
                {
                    Console.WriteLine(*(int*) (col[1].ptr + i * Marshal.SizeOf<int>()));
                }
            }
        }
    }
}
