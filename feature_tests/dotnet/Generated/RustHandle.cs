using System;
using System.Threading;

namespace Somelib.Diplomat;

#nullable enable

/// Frees a Rust-owned <typeparamref name="T"/> by calling its native destructor.
internal unsafe delegate void RustDestructor<T>(T* ptr) where T : unmanaged;

/// <summary>
/// Tracks the native lifetime and active borrow mode for one opaque value.
/// The initial claim belongs to the wrapper. Each borrow lease adds one claim.
/// </summary>
/// <remarks>
/// When the last claim ends, this runs the native destructor before releasing
/// the value's dependency edges.
/// </remarks>
internal sealed unsafe class RustHandle<T> where T : unmanaged
{
    private T* _ptr;
    private readonly RustDestructor<T>? _destructor;
    private object[] _edges;
    private readonly BorrowKind _capability;
    private int _refCount = 1;
    private int _borrowState;

    private RustHandle(
        T* ptr,
        RustDestructor<T>? destructor,
        BorrowKind capability,
        object[] edges
    )
    {
        _ptr = ptr;
        _destructor = destructor;
        _capability = capability;
        _edges = CaptureEdges(edges);
    }

    /// The C# side owns the pointer and will run its destructor on release.
    internal static RustHandle<T> Owned(T* ptr, RustDestructor<T> destructor) =>
        new RustHandle<T>(ptr, destructor, BorrowKind.Exclusive, System.Array.Empty<object>());

    /// Owned handle that also roots pins and/or borrow leases in <paramref name="edges"/>.
    internal static RustHandle<T> Owned(T* ptr, RustDestructor<T> destructor, object[] edges) =>
        new RustHandle<T>(ptr, destructor, BorrowKind.Exclusive, edges);

    /// Rust still owns the pointer; release never runs a destructor.
    internal static RustHandle<T> Borrowed(T* ptr, BorrowKind capability) =>
        new RustHandle<T>(ptr, null, capability, System.Array.Empty<object>());

    /// Borrowed handle that also roots keep-alive edges.
    internal static RustHandle<T> Borrowed(T* ptr, BorrowKind capability, object[] edges) =>
        new RustHandle<T>(ptr, null, capability, edges);

    internal T* Ptr => _ptr;

    private static object[] CaptureEdges(object[] edges)
    {
        for (int i = 0; i < edges.Length; i++)
        {
            if (edges[i] is IBorrowLease lease)
            {
                edges[i] = lease.Transfer();
            }
        }

        return edges;
    }

    /// True once this handle's native pointer has been cleared (refcount hit zero,
    /// or a borrowed handle was never assigned).
    internal bool IsNull => _ptr is null;

    internal BorrowLease<T> BorrowShared() => Acquire(BorrowKind.Shared);

    internal BorrowLease<T> BorrowExclusive()
    {
        if (_capability == BorrowKind.Shared)
        {
            throw new InvalidOperationException("This wrapper only carries a shared borrow.");
        }

        return Acquire(BorrowKind.Exclusive);
    }

    private BorrowLease<T> Acquire(BorrowKind kind)
    {
        RetainClaim();
        bool stateAcquired = false;
        try
        {
            AcquireBorrowState(kind);
            stateAcquired = true;
            return new BorrowLease<T>(this, kind);
        }
        catch
        {
            if (stateAcquired)
            {
                ReleaseBorrowState(kind);
            }
            Decrement();
            throw;
        }
    }

    private void RetainClaim()
    {
        while (true)
        {
            int current = Volatile.Read(ref _refCount);
            if (current == 0)
            {
                throw new ObjectDisposedException(typeof(T).Name);
            }

            if (Interlocked.CompareExchange(ref _refCount, current + 1, current) == current)
            {
                return;
            }
        }
    }

    private void AcquireBorrowState(BorrowKind kind)
    {
        if (kind == BorrowKind.Exclusive)
        {
            if (Interlocked.CompareExchange(ref _borrowState, -1, 0) != 0)
            {
                throw new InvalidOperationException("Another borrow is already active.");
            }

            return;
        }

        while (true)
        {
            int current = Volatile.Read(ref _borrowState);
            if (current < 0)
            {
                throw new InvalidOperationException("An exclusive borrow is already active.");
            }

            if (Interlocked.CompareExchange(ref _borrowState, current + 1, current) == current)
            {
                return;
            }
        }
    }

    internal void ReleaseBorrow(BorrowKind kind)
    {
        ReleaseBorrowState(kind);
        Decrement();
    }

    private void ReleaseBorrowState(BorrowKind kind)
    {
        if (kind == BorrowKind.Shared)
        {
            Interlocked.Decrement(ref _borrowState);
        }
        else
        {
            Volatile.Write(ref _borrowState, 0);
        }
    }

    /// Releases this wrapper's own owner reference.
    internal void Release() => Decrement();

    private void Decrement()
    {
        if (Interlocked.Decrement(ref _refCount) != 0)
        {
            return;
        }

        T* ptr = _ptr;
        _ptr = null;
        if (ptr != null && _destructor is not null)
        {
            _destructor(ptr);
        }

        object[] edges = _edges;
        _edges = System.Array.Empty<object>();
        foreach (object edge in edges)
        {
            (edge as IDisposable)?.Dispose();
        }
    }

}