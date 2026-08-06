using System;
using System.Runtime.CompilerServices;
using Somelib;
using Xunit;

namespace Somelib.FeatureTests;

public class FinalizerOrderTests
{
    [MethodImpl(MethodImplOptions.NoInlining
#if !NETFRAMEWORK
        | MethodImplOptions.AggressiveOptimization
#endif
    )]
    private static void CreateAndForgetBatch(int count)
    {
        var sources = new FinalizerOrderSource[count];
        var dependents = new FinalizerOrderDependent[count];

        // Register every source finalizer before the dependent finalizers. Each
        // generated dependent still has the normal strong `_edges` reference.
        for (int i = 0; i < count; i++)
        {
            sources[i] = FinalizerOrderSource.Create();
        }

        for (int i = 0; i < count; i++)
        {
            dependents[i] = sources[i].MakeDependent();
            Assert.True(dependents[i].ReadsSource());
        }

        // Intentionally do not call Dispose on either generated wrapper.
        GC.KeepAlive(dependents);
    }

    private static void DrainFinalizers(Func<bool> complete)
    {
        for (int attempt = 0; attempt < 20 && !complete(); attempt++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
    }

    [Fact]
    public void GeneratedEdges_DoNotGuaranteeOwnedBorrowingFinalizerOrder()
    {
        const int pairCount = 100_000;

        GC.Collect();
        GC.WaitForPendingFinalizers();
        FinalizerOrderSource.ResetProbe();

        CreateAndForgetBatch(pairCount);
        DrainFinalizers(
            () =>
                FinalizerOrderSource.SourceDrops() == (ulong)pairCount
                && FinalizerOrderSource.DependentDrops() == (ulong)pairCount
        );

        Assert.Equal((ulong)pairCount, FinalizerOrderSource.SourceDrops());
        Assert.Equal((ulong)pairCount, FinalizerOrderSource.DependentDrops());
        Assert.True(
            FinalizerOrderSource.BadOrderDrops() > 0,
            "Expected at least one source finalizer to run before its owned dependent finalizer"
        );
    }
}
