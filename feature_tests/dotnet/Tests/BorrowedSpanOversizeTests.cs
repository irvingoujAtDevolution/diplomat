using System;
using System.Runtime.CompilerServices;
using System.Threading;
using Somelib.Diplomat;
using Xunit;

namespace Somelib.FeatureTests;

// Repro for the oversize DiplomatBorrowedSpan constructor path.
//
// Call sites build borrow-lease edges *before* `new DiplomatBorrowedSpan(...)`.
// The constructor normally converts those leases to versioned tokens. If construction
// throws because `len` does not fit a .NET Span — without disposing the edges
// and without SuppressFinalize on the half-built object — then:
//   1. `_edges` is still null
//   2. ~DiplomatBorrowedSpan NREs on the foreach, catch {} swallows it
//   3. the borrow leases are never released → parent native alloc leaks
//
// We use a pure-managed counting edge so this is deterministic and does not
// fight process-global native drop counters from other tests.
public class BorrowedSpanOversizeTests
{
    private sealed class CountingEdge : IDisposable
    {
        private int _disposeCount;

        public int DisposeCount => Volatile.Read(ref _disposeCount);

        public void Dispose() => Interlocked.Increment(ref _disposeCount);
    }

    [MethodImpl(MethodImplOptions.NoInlining
#if !NETFRAMEWORK
        | MethodImplOptions.AggressiveOptimization
#endif
    )]
    private static void ForceGcUntil(Func<bool> condition)
    {
        for (int i = 0; i < 50 && !condition(); i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }
    }

    // Pointer is never dereferenced on the oversize path; only len + edges matter.
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static unsafe void ConstructOversizeBorrowedSpan(IDisposable?[] edges)
    {
        nuint oversize = (nuint)int.MaxValue + 1;
        _ = new DiplomatBorrowedSpan<byte>(null, oversize, edges);
    }

    [Fact]
    public void OversizeConstructor_ThrowsAndDisposesBorrowEdgesImmediately()
    {
        var edge = new CountingEdge();
        IDisposable?[] edges = new IDisposable?[] { edge };

        IndexOutOfRangeException ex = Assert.Throws<IndexOutOfRangeException>(() =>
            ConstructOversizeBorrowedSpan(edges)
        );
        Assert.Contains("too large", ex.Message);

        // Must not require a GC pass: failed construction owns the cleanup duty
        // for edges it was handed (same shape as RustVec's oversize path).
        Assert.Equal(1, edge.DisposeCount);
    }

    /// <summary>
    /// Guards the NRE-swallowed-in-finalizer failure mode: even after the
    /// half-built object becomes unreachable, the borrow edge must end up
    /// disposed. A correct ctor does this before throw; a broken one never does.
    /// </summary>
    [Fact]
    public void OversizeConstructor_FailedObjectDoesNotLeakRetainAcrossGc()
    {
        var edge = new CountingEdge();
        IDisposable?[] edges = new IDisposable?[] { edge };

        try
        {
            ConstructOversizeBorrowedSpan(edges);
        }
        catch (IndexOutOfRangeException)
        {
            // expected once the length guard exists
        }

        // Drop managed refs that could keep the edge reachable outside the span.
        edges = null!;

        ForceGcUntil(() => edge.DisposeCount >= 1);

        Assert.Equal(1, edge.DisposeCount);
    }
}
