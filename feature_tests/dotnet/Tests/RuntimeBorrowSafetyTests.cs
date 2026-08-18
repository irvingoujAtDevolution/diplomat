using System;
using System.Threading;
using System.Threading.Tasks;
using Somelib;
using Xunit;

namespace Somelib.FeatureTests;

[Collection(RcSharedNativeStateCollection.Name)]
public sealed class RuntimeBorrowSafetyTests
{
    private static async Task AwaitWithTimeout(Task task)
    {
        Task completed = await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(5)));
        Assert.Same(task, completed);
        await task;
    }

    [Fact]
    public async Task ConcurrentSharedCalls_AreAllowedButMutableCallsFailFast()
    {
        BorrowSafetyProbe.ResetSharedCall();
        using BorrowSafetyProbe probe = BorrowSafetyProbe.Create();
        Task heldCall = Task.Factory.StartNew(
            probe.HoldShared,
            CancellationToken.None,
            TaskCreationOptions.LongRunning,
            TaskScheduler.Default
        );

        try
        {
            Assert.True(
                SpinWait.SpinUntil(BorrowSafetyProbe.SharedCallEntered, TimeSpan.FromSeconds(5))
            );
            Assert.True(probe.PingShared());
            Assert.Throws<InvalidOperationException>(() => probe.PingMutable());
        }
        finally
        {
            BorrowSafetyProbe.ReleaseSharedCall();
            await AwaitWithTimeout(heldCall);
        }
    }

    [Fact]
    public async Task ConcurrentMutableCall_RejectsSharedAndMutableCalls()
    {
        BorrowSafetyProbe.ResetMutableCall();
        using BorrowSafetyProbe probe = BorrowSafetyProbe.Create();
        Task heldCall = Task.Factory.StartNew(
            probe.HoldMutable,
            CancellationToken.None,
            TaskCreationOptions.LongRunning,
            TaskScheduler.Default
        );

        try
        {
            Assert.True(
                SpinWait.SpinUntil(BorrowSafetyProbe.MutableCallEntered, TimeSpan.FromSeconds(5))
            );
            Assert.Throws<InvalidOperationException>(() => probe.PingShared());
            Assert.Throws<InvalidOperationException>(() => probe.PingMutable());
        }
        finally
        {
            BorrowSafetyProbe.ReleaseMutableCall();
            await AwaitWithTimeout(heldCall);
        }

        Assert.True(probe.PingMutable());
    }
}
