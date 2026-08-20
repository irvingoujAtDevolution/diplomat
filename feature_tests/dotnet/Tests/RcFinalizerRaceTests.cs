using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Somelib;
using Xunit;

namespace Somelib.FeatureTests;

// Regression coverage for the `RustHandle<T>._refCount` / GC-finalizer-thread
// race — exercised WITHOUT any user-side thread-safety violation.
//
// Every operation THIS test performs is confined to one thread: it creates
// one source and repeatedly creates-and-abandons borrowed views of it. But
// each abandoned view is finalized on the GC finalizer thread, and that
// finalizer disposes the view's DependencyToken, which calls `Decrement()`
// on the SOURCE's handle. Meanwhile the user thread's next `View()` call
// runs `Retain()` on that same handle. The finalizer thread is therefore an
// unavoidable second lifecycle thread on the shared handle.
//
// With a plain unfenced `_refCount++` / `--_refCount` (the bug), interleaved
// increments/decrements lose updates:
//   * lost Retain()  -> refcount hits zero early -> the native source is
//     destroyed while the live, undisposed source wrapper still owns its
//     reference (use-after-free);
//   * lost Release() -> refcount never reaches zero -> the native source is
//     never destroyed (leak).
// Either outcome trips one of the two assertions below. The Interlocked
// CAS/Decrement in `RustHandle.cs` makes both lanes impossible.
[Collection(RcSharedNativeStateCollection.Name)]
public class RcFinalizerRaceTests
{
    [MethodImpl(MethodImplOptions.NoInlining
#if !NETFRAMEWORK
        | MethodImplOptions.AggressiveOptimization
#endif
    )]
    private static void CreateAndAbandonView(RcSource source)
    {
        // Retain()s the source's handle (+1, user thread). The abandoned
        // wrapper is later finalized on the GC finalizer thread, whose
        // cleanup Decrement()s the same handle (-1).
        source.View();
    }

    private static void DrainFinalizers()
    {
        for (int i = 0; i < 20; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
        GC.Collect();
    }

    [Fact]
    public void RetainOnUserThread_WhileFinalizerThreadReleases_KeepsRefCountConsistent()
    {
        // Drain leftovers, then root the owner against parallel GC.Collect.
        DrainFinalizers();
        RcSource.ResetDropStats();

        RcSource source = RcSource.Create(42);
        GCHandle root = GCHandle.Alloc(source);
        try
        {
            const int Iterations = 400_000;
            bool prematureDestruction = false;
            for (int i = 0; i < Iterations; i++)
            {
                // Detect a lost Retain() as soon as it manifests, and bail out
                // before the next View() call would P/Invoke into freed memory.
                if (RcSource.DropCount() != 0)
                {
                    prematureDestruction = true;
                    break;
                }

                CreateAndAbandonView(source);

                // Kick dead views onto the finalizer queue WITHOUT waiting, so
                // the finalizer thread drains (and Decrement()s) concurrently
                // with this thread's ongoing Retain()s.
                if ((i & 2047) == 0)
                {
                    GC.Collect(0);
                }
            }

            // Let every abandoned view finish finalizing so all decrements have
            // been applied before examining the final state.
            DrainFinalizers();

            Assert.False(
                prematureDestruction,
                "lost Retain(): the native RcSource was destroyed while the owning wrapper " +
                "was still live and undisposed (use-after-free window)"
            );
            Assert.Equal(0ul, RcSource.DropCount());

            // Every view token has been released; the only remaining reference
            // is the source wrapper's own. Releasing it must destroy the native
            // value exactly once.
            source.Dispose();
            DrainFinalizers();

            ulong drops = RcSource.DropCount();
            Assert.True(
                drops == 1ul,
                $"lost Release(): expected exactly one native destruction after the last " +
                $"reference was released, got {drops} — a finalizer-thread Decrement() and a " +
                $"user-thread Retain() overwrote each other (refcount corruption)"
            );
        }
        finally
        {
            if (root.IsAllocated)
            {
                root.Free();
            }
        }
    }
}
