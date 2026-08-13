using System;

namespace Somelib.Diplomat;

#nullable enable

// PROTOTYPE: lazy, single-lane native handle. Every opaque wrapper starts as
// a bare pointer + destructor, with no heap allocation at all. The first time
// something actually borrows FROM it (`Retain`), it lazily promotes itself
// into a shared `RustHandleState<T>` so the borrow can outlive this wrapper's
// own managed lifetime. A type nothing ever borrows from never allocates
// that state. Consumer thread-confined by design: no locks, no atomics — do
// not call Dispose/Retain for the same handle from two threads at once, and
// do not race a Dispose() against the finalizer.

/// Frees a Rust-owned <typeparamref name="T"/> by calling its native destructor.
internal unsafe delegate void RustDestructor<T>(T* ptr) where T : unmanaged;

/// One retained borrow-dependency token. A dependent holds one of these per
/// direct source it borrows from (see <see cref="RustHandle{T}.Retain"/>) and
/// releases it - exactly once, from its own `_edges` cleanup.
internal interface IRustHandleDependency
{
    void Release();
}

/// <summary>
/// The shared, reference-counted state a <see cref="RustHandle{T}"/> lazily
/// promotes itself into the first time something retains a dependency on it.
/// Owns the pointer/destructor and whatever edges (pins and/or
/// <see cref="IRustHandleDependency"/> tokens) the promoting wrapper had
/// already collected — moved in wholesale at promotion time (see
/// <see cref="RustHandle{T}.Retain"/>).
/// </summary>
/// <remarks>
/// The count starts at 1 (the owning wrapper's own reference); each
/// <see cref="Retain"/> call bumps it and hands back a token the new
/// dependent releases later. When the count reaches zero, this runs the
/// native destructor first, then disposes/releases every edge - never the
/// other way around, so an owned-but-borrowing dependent's own destructor
/// always finishes before whatever it borrowed from can be destroyed.
/// </remarks>
internal sealed unsafe class RustHandleState<T> where T : unmanaged
{
    private T* _ptr;
    private readonly RustDestructor<T>? _destructor;
    private object[] _edges;
    private int _refCount = 1;

    internal RustHandleState(T* ptr, RustDestructor<T>? destructor, object[] edges)
    {
        _ptr = ptr;
        _destructor = destructor;
        _edges = edges;
    }

    internal T* Ptr => _ptr;

    /// Bumps the count for one new direct dependent and returns its token.
    internal IRustHandleDependency Retain()
    {
        _refCount++;
        return new DependencyToken(this);
    }

    /// Releases the owning wrapper's single reference.
    internal void ReleaseOwner() => Decrement();

    private void Decrement()
    {
        if (--_refCount != 0)
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
            (edge as IRustHandleDependency)?.Release();
        }
    }

    private sealed class DependencyToken : IRustHandleDependency
    {
        private RustHandleState<T>? _owner;

        internal DependencyToken(RustHandleState<T> owner)
        {
            _owner = owner;
        }

        public void Release()
        {
            RustHandleState<T>? owner = _owner;
            _owner = null;
            owner?.Decrement();
        }
    }
}

/// A native handle for one opaque wrapper instance: a raw pointer plus the
/// destructor that frees it, lazily promoting to a shared
/// <see cref="RustHandleState{T}"/> only if something actually retains a
/// dependency on it (see <see cref="Retain"/>). Every opaque type uses this
/// same struct - there is no separate reference-counted lane.
internal unsafe struct RustHandle<T> where T : unmanaged
{
    private T* _ptr;
    private RustDestructor<T>? _destructor;
    private RustHandleState<T>? _state;

    private RustHandle(T* ptr, RustDestructor<T>? destructor)
    {
        _ptr = ptr;
        _destructor = destructor;
        _state = null;
    }

    /// The C# side owns the pointer and will run its destructor on release.
    internal static RustHandle<T> Owned(T* ptr, RustDestructor<T> destructor) =>
        new RustHandle<T>(ptr, destructor);

    /// Rust still owns the pointer; release never runs a destructor.
    internal static RustHandle<T> Borrowed(T* ptr) => new RustHandle<T>(ptr, null);

    internal T* Ptr => _ptr;

    /// True once this handle has been released (or was never assigned).
    internal bool IsNull => _ptr is null;

    /// Retains this handle's native resource for a new direct dependent,
    /// returning the token that dependent must release exactly once, from
    /// its own cleanup. On the FIRST retain ever made against this handle,
    /// this lazily promotes it into a shared <see cref="RustHandleState{T}"/>
    /// - moving `edges` (the promoting wrapper's own `_edges` field) into
    /// that state and clearing it, since those edges are now released by the
    /// state's own cleanup instead of the wrapper's.
    internal IRustHandleDependency Retain(ref object[] edges)
    {
        if (_ptr is null)
        {
            throw new ObjectDisposedException(typeof(T).Name);
        }

        if (_state is null)
        {
            _state = new RustHandleState<T>(_ptr, _destructor, edges);
            edges = System.Array.Empty<object>();
        }

        return _state.Retain();
    }

    /// Releases this handle's own ("owner") reference: runs the destructor
    /// directly if never promoted, or defers to the shared state's own
    /// refcounted release otherwise.
    internal void Release()
    {
        if (_state is not null)
        {
            _state.ReleaseOwner();
        }
        else if (_destructor is not null && _ptr is not null)
        {
            _destructor(_ptr);
        }
    }
}