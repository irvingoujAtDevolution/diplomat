using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;

const int pairCount = 100_000;

Console.WriteLine($".NET {Environment.Version}");
Console.WriteLine($"Creating {pairCount:N0} finalizable pairs per model...");

EdgeCounters.Reset();
CreateEdgeOnlyBatch(pairCount);
DrainFinalizers(() => EdgeCounters.DependentDrops == pairCount);

Console.WriteLine(
    $"edge-only: source={EdgeCounters.SourceDrops:N0}, "
        + $"dependent={EdgeCounters.DependentDrops:N0}, "
        + $"source-first={EdgeCounters.BadOrder:N0}"
);

RefCountCounters.Reset();
CreateRefCountedBatch(pairCount);
DrainFinalizers(() => RefCountCounters.DependentDrops == pairCount);

Console.WriteLine(
    $"refcount:  source={RefCountCounters.SourceDrops:N0}, "
        + $"dependent={RefCountCounters.DependentDrops:N0}, "
        + $"source-first={RefCountCounters.BadOrder:N0}"
);

if (EdgeCounters.BadOrder == 0)
{
    Console.Error.WriteLine("The edge-only ordering failure did not reproduce.");
    return 2;
}

if (RefCountCounters.BadOrder != 0)
{
    Console.Error.WriteLine("The refcounted model allowed a source-first release.");
    return 3;
}

return 0;

[MethodImpl(MethodImplOptions.NoInlining)]
static void CreateEdgeOnlyBatch(int count)
{
    var sources = new EdgeOnlySource[count];
    var dependents = new EdgeOnlyDependent[count];

    // Register every source finalizer before any dependent finalizer. This does
    // not create an invalid program: each dependent still strongly roots its
    // source through _edges. It only makes the unspecified ordering observable.
    for (int i = 0; i < count; i++)
    {
        sources[i] = new EdgeOnlySource();
    }

    for (int i = 0; i < count; i++)
    {
        dependents[i] = new EdgeOnlyDependent(sources[i]);
    }

    GC.KeepAlive(dependents);
}

[MethodImpl(MethodImplOptions.NoInlining)]
static void CreateRefCountedBatch(int count)
{
    var sources = new RefCountedSource[count];
    var dependents = new RefCountedDependent[count];

    for (int i = 0; i < count; i++)
    {
        sources[i] = new RefCountedSource();
    }

    for (int i = 0; i < count; i++)
    {
        dependents[i] = new RefCountedDependent(sources[i]);
    }

    GC.KeepAlive(dependents);
}

static void DrainFinalizers(Func<bool> complete)
{
    for (int attempt = 0; attempt < 20 && !complete(); attempt++)
    {
        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
        GC.WaitForPendingFinalizers();
    }

    GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
    GC.WaitForPendingFinalizers();
}

sealed class OrderState
{
    internal int SourceFinalized;
}

sealed class EdgeOnlySource
{
    internal readonly OrderState State = new();

    ~EdgeOnlySource()
    {
        Volatile.Write(ref State.SourceFinalized, 1);
        Interlocked.Increment(ref EdgeCounters.SourceDrops);
    }
}

sealed class EdgeOnlyDependent
{
    // This is the same kind of strong managed edge generated wrappers used.
    private readonly object[] _edges;

    internal EdgeOnlyDependent(EdgeOnlySource source)
    {
        _edges = [source];
    }

    ~EdgeOnlyDependent()
    {
        var source = (EdgeOnlySource)_edges[0];
        if (Volatile.Read(ref source.State.SourceFinalized) != 0)
        {
            Interlocked.Increment(ref EdgeCounters.BadOrder);
        }

        Interlocked.Increment(ref EdgeCounters.DependentDrops);
        GC.KeepAlive(_edges);
    }
}

static class EdgeCounters
{
    internal static int SourceDrops;
    internal static int DependentDrops;
    internal static int BadOrder;

    internal static void Reset()
    {
        SourceDrops = 0;
        DependentDrops = 0;
        BadOrder = 0;
    }
}

sealed class SourceSafeHandle : SafeHandle
{
    private readonly OrderState _state;

    internal SourceSafeHandle(OrderState state)
        : base(IntPtr.Zero, ownsHandle: true)
    {
        _state = state;
        SetHandle((IntPtr)1);
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Volatile.Write(ref _state.SourceFinalized, 1);
        Interlocked.Increment(ref RefCountCounters.SourceDrops);
        handle = IntPtr.Zero;
        return true;
    }
}

sealed class DependentSafeHandle : SafeHandle
{
    private readonly OrderState _state;
    private SourceSafeHandle? _source;
    private bool _sourceRefAdded;

    internal DependentSafeHandle(OrderState state, SourceSafeHandle source)
        : base(IntPtr.Zero, ownsHandle: true)
    {
        _state = state;

        bool added = false;
        source.DangerousAddRef(ref added);
        _source = source;
        _sourceRefAdded = added;

        SetHandle((IntPtr)1);
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        try
        {
            if (Volatile.Read(ref _state.SourceFinalized) != 0)
            {
                Interlocked.Increment(ref RefCountCounters.BadOrder);
            }

            Interlocked.Increment(ref RefCountCounters.DependentDrops);
            handle = IntPtr.Zero;
            return true;
        }
        finally
        {
            if (_sourceRefAdded)
            {
                _sourceRefAdded = false;
                _source!.DangerousRelease();
                _source = null;
            }
        }
    }
}

sealed class RefCountedSource
{
    internal readonly OrderState State = new();
    internal readonly SourceSafeHandle Handle;

    internal RefCountedSource()
    {
        Handle = new SourceSafeHandle(State);
    }
}

sealed class RefCountedDependent
{
    // Keep the same managed edge as the edge-only model. The only difference
    // is the explicit SafeHandle reference that orders native destruction.
    private readonly object[] _edges;
    private readonly DependentSafeHandle _handle;

    internal RefCountedDependent(RefCountedSource source)
    {
        _edges = [source];
        _handle = new DependentSafeHandle(source.State, source.Handle);
    }
}

static class RefCountCounters
{
    internal static int SourceDrops;
    internal static int DependentDrops;
    internal static int BadOrder;

    internal static void Reset()
    {
        SourceDrops = 0;
        DependentDrops = 0;
        BadOrder = 0;
    }
}
