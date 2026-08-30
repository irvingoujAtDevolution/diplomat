using System;
using System.Collections.Generic;
using System.Threading;
using Somelib;
using Xunit;

namespace Somelib.FeatureTests;

// One RustHandle under fire from many threads at once. Asserts the lifecycle
// invariants hold under contention: every operation either succeeds or throws
// the documented exception, and the native value is destroyed exactly once.
[Collection(RcSharedNativeStateCollection.Name)]
public class HandleConcurrencyStressTests
{
    private const int Threads = 8;

    private static void RunOnThreads(Action<int> body)
    {
        List<Exception> unexpected = new List<Exception>();
        Thread[] threads = new Thread[Threads];
        using ManualResetEventSlim go = new ManualResetEventSlim(false);
        for (int t = 0; t < Threads; t++)
        {
            int id = t;
            threads[t] = new Thread(() =>
            {
                go.Wait();
                try
                {
                    body(id);
                }
                catch (Exception e)
                {
                    lock (unexpected)
                    {
                        unexpected.Add(e);
                    }
                }
            });
            threads[t].IsBackground = true;
            threads[t].Start();
        }
        go.Set();
        foreach (Thread t in threads)
        {
            t.Join();
        }
        Assert.Empty(unexpected);
    }

    [Fact]
    public void SharedAndExclusiveCalls_UnderContention_OnlyDocumentedExceptions()
    {
        RcSource.ResetDropStats();
        using (RcSource source = RcSource.Create(7))
        {
            RunOnThreads(id =>
            {
                for (int i = 0; i < 2000; i++)
                {
                    try
                    {
                        if ((i + id) % 3 == 0)
                        {
                            source.PingMutable();
                        }
                        else
                        {
                            Assert.Equal(7ul, source.Id());
                        }
                    }
                    catch (InvalidOperationException)
                    {
                        // borrow contention: the documented outcome
                    }
                }
            });
            Assert.Equal(0ul, RcSource.DropCount());
            Assert.True(source.PingMutable());
        }
        Assert.Equal(1ul, RcSource.DropCount());
    }

    [Fact]
    public void DisposeRacingCalls_DropsExactlyOnce_ThenObjectDisposed()
    {
        for (int round = 0; round < 20; round++)
        {
            RcSource.ResetDropStats();
            RcSource source = RcSource.Create(7);
            RunOnThreads(id =>
            {
                for (int i = 0; i < 200; i++)
                {
                    try
                    {
                        if (id == 0 && i == 50)
                        {
                            source.Dispose();
                        }
                        else
                        {
                            source.Id();
                        }
                    }
                    catch (ObjectDisposedException)
                    {
                    }
                    catch (InvalidOperationException)
                    {
                    }
                }
            });
            Assert.Equal(1ul, RcSource.DropCount());
            Assert.Throws<ObjectDisposedException>(() => source.Id());
        }
    }

    [Fact]
    public void DoubleDisposeRacingViews_DropsSourceExactlyOnce()
    {
        for (int round = 0; round < 10; round++)
        {
            RcSource.ResetDropStats();
            RcSource source = RcSource.Create(7);
            RunOnThreads(id =>
            {
                for (int i = 0; i < 100; i++)
                {
                    try
                    {
                        if (i % 10 == 9)
                        {
                            source.Dispose();
                        }
                        else
                        {
                            using RcSource view = source.View();
                            view.Id();
                        }
                    }
                    catch (ObjectDisposedException)
                    {
                    }
                    catch (InvalidOperationException)
                    {
                    }
                }
            });
            GC.Collect();
            GC.WaitForPendingFinalizers();
            Assert.Equal(1ul, RcSource.DropCount());
        }
    }
}
