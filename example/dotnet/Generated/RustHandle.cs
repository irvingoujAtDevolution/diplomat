using System;

namespace Somelib.Diplomat;

#nullable enable

/// <summary>
/// Frees a Rust-owned <typeparamref name="T"/> by calling its native
/// destructor. Held by an owned handle (<see cref="RustHandle{T}"/> or
/// <see cref="RcRustHandle{T}"/>) so it can release the pointer without
/// knowing the concrete type.
/// </summary>
internal unsafe delegate void RustDestructor<T>(T* ptr) where T : unmanaged;

/// <summary>
/// A single retained borrow-dependency token: a dependent's native resource
/// holds one of these per direct source it borrows from (see
/// <see cref="RcRustHandleState{T}.Retain"/>), and releases it - exactly
/// once, after running its own Rust destructor if it has one - from its own
/// cleanup. This is what defers a source's physical destruction until every
/// direct dependent (and the source's own wrapper) has let go of it.
/// </summary>
/// <remarks>
/// Calling <see cref="Release"/> is a lifecycle edge (dependent construction/
/// cleanup), not a per-call operation, so implementations may synchronize it
/// internally. See <see cref="RcRustHandleState{T}"/> for the full contract
/// and what is (and is not) synchronized.
/// </remarks>
internal interface IRustHandleDependency
{
    void Release();
}

/// <summary>
/// A raw pointer plus the Rust destructor that frees it, with no reference
/// counting at all. This is the plain, zero-allocation lane every opaque
/// wrapper uses UNLESS the generator's selective borrow-source analysis
/// classified it as an actual RC source (see <see cref="RcRustHandle{T}"/>
/// and <c>lifetime.rs</c> in the generator) - i.e. the overwhelming majority
/// of opaque types in a typical run, since most opaques are never borrowed
/// from by anything else.
/// </summary>
/// <remarks>
/// A type that itself borrows from another opaque (or pins one of its own
/// input buffers) but that nothing else ever borrows FROM still uses this
/// plain handle for its own pointer - it just carries a couple of extra
/// fields alongside it to hold/release what it borrowed (see the generated
/// wrapper's own <c>_dependencies</c>/<c>_pins</c> fields in
/// <c>opaque.impl.cs.jinja</c>), rather than allocating a shared, retainable
/// <see cref="RcRustHandleState{T}"/> nothing will ever actually retain.
/// </remarks>
internal readonly unsafe struct RustHandle<T> where T : unmanaged
{
    private readonly T* _ptr;
    private readonly RustDestructor<T>? _destructor;

    private RustHandle(T* ptr, RustDestructor<T>? destructor)
    {
        _ptr = ptr;
        _destructor = destructor;
    }

    /// <summary>The C# side owns the pointer and will run its destructor on release.</summary>
    internal static RustHandle<T> Owned(T* ptr, RustDestructor<T> destructor) =>
        new RustHandle<T>(ptr, destructor);

    /// <summary>Rust still owns the pointer; release never runs a destructor.</summary>
    internal static RustHandle<T> Borrowed(T* ptr) => new RustHandle<T>(ptr, null);

    internal T* Ptr => _ptr;

    /// <summary>True once this handle has been released (or was never assigned).</summary>
    internal bool IsNull => _ptr is null;

    /// <summary>
    /// Runs the destructor (if this handle owns one). Not reference-counted
    /// and not internally synchronized in any way: this method itself
    /// performs no locking, and calling it twice concurrently for the same
    /// live handle value - e.g. two genuinely overlapping <c>Dispose()</c>
    /// calls, or a <c>Dispose()</c> that races the finalizer rather than
    /// merely following it - is a double-destroy; there is no internal guard
    /// here (or in the owning wrapper's own, non-RC <c>Cleanup()</c>) that
    /// would catch that.
    /// </summary>
    /// <remarks>
    /// Ordinary sequential use is safe only because of a CONSUMER contract,
    /// not an implementation guarantee: every opaque wrapper built on this
    /// plain handle (i.e. every type the generator's selective borrow-source
    /// analysis did NOT classify as an actual RC source - see
    /// <c>lifetime.rs</c>) thread-confines its own lifecycle - do not call
    /// <c>Dispose()</c> for the same instance from two threads at once, and
    /// do not let the finalizer thread race an in-flight <c>Dispose()</c>
    /// call for that instance. Repeated <c>Dispose()</c> calls made one after
    /// another (including a clean finalizer pass once <c>Dispose()</c> has
    /// already returned) work because the wrapper's <c>Cleanup()</c> checks
    /// <see cref="IsNull"/> before calling this and clears its own field to
    /// <c>default</c> right afterwards - but that check-then-clear is not
    /// atomic, so it does not make a genuinely concurrent call safe. This is
    /// intentional: we do NOT add any lock-free or lock-based cleanup guard
    /// to this plain lane. That cost belongs only on the actual RC lifecycle
    /// edges (see <see cref="RcRustHandleState{T}"/>), where a finalizer
    /// racing the application thread is inherent to finalization itself, not
    /// a threading choice the consumer could avoid by construction.
    /// </remarks>
    internal void Release()
    {
        if (_destructor != null && _ptr != null)
        {
            _destructor(_ptr);
        }
    }
}

/// <summary>
/// The reference-counted native resource state shared by an
/// <see cref="RcRustHandle{T}"/> and every dependent that borrows from it.
/// Allocated only for opaque types the generator's selective borrow-source
/// analysis classified as an actual RC source - i.e. at least one OTHER
/// generated wrapper's return value retains a borrow of this type (see
/// <c>lifetime.rs</c> in the generator). Every other opaque type uses the
/// plain, non-allocating <see cref="RustHandle{T}"/> instead.
/// </summary>
/// <remarks>
/// <para>
/// One state object is created per handle-owning C# wrapper instance. The
/// count starts at 1, standing for that wrapper's own ("owner") reference.
/// Each direct dependent that borrows from the wrapper calls
/// <see cref="Retain"/>, which bumps the count and hands back a distinct
/// <see cref="IRustHandleDependency"/> token for that one dependent to
/// release later - not the wrapper itself, and not this state object
/// directly - so the shared native resource stays alive by refcount alone,
/// independent of which wrapper's managed lifetime (explicit
/// <c>Dispose()</c> or finalizer) happens to end first.
/// </para>
/// <para>
/// <b>Why this needs synchronization at all.</b> Finalizers run
/// concurrently with whatever the application is doing, on a dedicated
/// finalizer thread, regardless of how carefully single-threaded the user's
/// own code is. A dependent's finalizer can call
/// <see cref="IRustHandleDependency.Release"/> on a source's shared state at
/// the exact same instant the application thread calls
/// <c>Dispose()</c> on that same source (or on a second dependent sharing
/// the same source), and both paths decrement the very same shared count.
/// A plain, unguarded <see cref="int"/> decrement is not safe under that
/// interleaving: two racing decrements can lose an update, or two threads
/// can each observe the count reaching zero and both run the native
/// destructor. This class fixes that race with a single <c>lock</c>
/// guarding the plain <c>int</c> count itself - it is intentionally NOT an
/// atomic/interlocked counter, and generated wrappers still add zero
/// synchronization to hot per-call code (P/Invoke calls, property getters,
/// etc.). Only the lifecycle edges below ever take the lock:
/// <see cref="Retain"/>, <see cref="ReleaseOwner"/>, and each dependency
/// token's own <c>Release()</c>.
/// </para>
/// <para>
/// The owning wrapper's reference and each dependent's reference are
/// released through two different, separately one-shot-guarded paths so a
/// bug in one can never silently paper over a bug in the other:
/// </para>
/// <list type="bullet">
/// <item>
/// <see cref="ReleaseOwner"/> is idempotent - safe to call more than once
/// (e.g. a repeated/racing <c>Dispose()</c>) - because only the *first* call
/// actually decrements the count. The generated wrapper's own
/// <c>Cleanup()</c> relies on this idempotency, rather than re-implementing
/// its own one-shot guard, to make a wrapper's owner reference release
/// exactly once no matter how many times or from how many threads
/// <c>Cleanup()</c> ends up running.
/// </item>
/// <item>
/// Each call to <see cref="Retain"/> hands back a brand-new token object
/// (see the private <c>DependencyToken</c> nested class) whose own
/// <c>Release()</c> is one-shot-guarded independently of every other token
/// and of the owner. This means an accidental duplicate <c>Release()</c>
/// call on the SAME token is always a safe no-op, while still requiring
/// every legitimately-retained token to be released before the count can
/// reach zero - a generic "count already &lt;= 0, do nothing" guard at the
/// shared-state level would instead risk silently masking a real
/// double-release bug, so this state does not rely on one: reaching a
/// non-positive count inside the shared decrement path always throws.
/// </item>
/// </list>
/// <para>
/// Exactly one of these releases will observe the count reaching zero
/// (guaranteed by the lock above). That one thread - and only that thread -
/// captures the pointer, destructor, dependency tokens, and pins under the
/// lock, clears them from this object, and then (after releasing the lock)
/// runs the native destructor, unpins this wrapper's own pinned input
/// buffers, and releases this wrapper's own retained dependencies, strictly
/// in that order. None of that cleanup work - running the Rust destructor,
/// disposing pins, or recursively releasing dependencies - ever runs while
/// holding the lock. This "destroy self, unpin self, then release what I
/// depend on" order is load-bearing for two separate hazards at once:
/// </para>
/// <list type="bullet">
/// <item>
/// It's what makes an owned-but-borrowing dependent's destructor always run
/// strictly before its source(s) can be physically destroyed, and - because
/// every level only ever deals with its own direct dependencies - the same
/// order cascades correctly through any depth of direct-only borrow edges
/// without the generator ever needing to compute a transitive closure.
/// </item>
/// <item>
/// It's what keeps this wrapper's OWN pinned input buffers alive/unmoved for
/// as long as this wrapper's OWN Rust destructor could still run - even when
/// that destructor call itself is deferred by an outstanding dependent, not
/// invoked by this object's own release call. Because pins live in this same
/// state object (not in a separately-released wrapper-level field), the
/// refcount reaching zero is the one and only moment either the destructor
/// or the unpin can happen, and the destructor always comes first.
/// </item>
/// </list>
/// <para>
/// <b>What this does NOT provide.</b> Generated wrappers make no promise of
/// concurrent-method-call safety: calling ordinary instance methods (or
/// mutating/reading the same wrapper) from two threads at once is still
/// undefined behavior, exactly as before. This is not a general-purpose
/// atomic-reference-counting (ARC) scheme - there is no per-native-call
/// retain/release, and hot paths remain lock-free. Only the handful of
/// lifecycle edges where finalizer-thread and application-thread work can
/// genuinely race - dependent construction, wrapper <c>Dispose()</c>/
/// finalizer, and dependency release - are synchronized, because those are
/// the only edges where concurrency is inherent (finalization) rather than
/// a user threading choice.
/// </para>
/// </remarks>
internal sealed unsafe class RcRustHandleState<T> where T : unmanaged
{
    private readonly object _gate = new object();
    private T* _ptr;
    private readonly RustDestructor<T>? _destructor;
    private IRustHandleDependency[] _dependencies;
    private object[] _pins;
    private int _refCount;
    private bool _ownerReleased;

    internal RcRustHandleState(T* ptr, RustDestructor<T>? destructor, IRustHandleDependency[] dependencies, object[] pins)
    {
        _ptr = ptr;
        _destructor = destructor;
        _dependencies = dependencies;
        _pins = pins;
        _refCount = 1;
    }

    /// <summary>
    /// The raw pointer, or null once the shared count has reached zero.
    /// Read without synchronization: per-call use of a handle (method calls,
    /// <c>AsFFI()</c>) is not itself synchronized against other calls on the
    /// SAME wrapper from other threads - see the type-level remarks - so
    /// there is no additional guarantee to provide here over a plain field
    /// read.
    /// </summary>
    internal T* Ptr => _ptr;

    /// <summary>
    /// Bumps the reference count for one new direct dependent and returns a
    /// token that dependent must release exactly once. Throws if the count
    /// has already reached zero OR the owner has already logically released
    /// its reference (<see cref="_ownerReleased"/>) - the source's native
    /// resource is already gone, or is unavoidably about to be. In practice
    /// every generated call site already guards this the same way one level
    /// up (a wrapper's own `DiplomatRetainDependency()` checks its own handle
    /// first - see `opaque.impl.cs.jinja`), so this check is defense in
    /// depth against ever reaching a retain through some other path once
    /// this state's owner has logically let go. This can only race with
    /// <see cref="ReleaseOwner"/> or another token's release reaching zero at
    /// the same instant, which the lock below resolves the same way a
    /// single-threaded caller would expect: either this retain is ordered
    /// first (and the racing release is deferred, exactly as if it had lost
    /// a single-threaded race) or the resource is already gone/going (and
    /// this call throws).
    /// </summary>
    internal IRustHandleDependency Retain()
    {
        lock (_gate)
        {
            if (_ownerReleased || _refCount <= 0)
            {
                throw new ObjectDisposedException(typeof(T).Name);
            }

            _refCount++;
        }

        return new DependencyToken(this);
    }

    /// <summary>
    /// Releases the owning wrapper's single reference. Idempotent: only the
    /// first call (from whichever thread makes it, however many times
    /// <c>Dispose()</c>/the finalizer end up running for this wrapper) does
    /// anything. See the type-level remarks for why this is deliberately a
    /// separate, self-guarded path from a dependency token's release.
    /// </summary>
    internal void ReleaseOwner()
    {
        lock (_gate)
        {
            if (_ownerReleased)
            {
                return;
            }

            _ownerReleased = true;
        }

        Decrement();
    }

    /// <summary>
    /// Decrements the shared count and, exactly once - on whichever single
    /// call (owner or dependency token) observes it reach zero - runs the
    /// cleanup described in the type-level remarks. Every caller of this
    /// method (<see cref="ReleaseOwner"/>, <c>DependencyToken.Release</c>)
    /// is already individually one-shot-guarded, so a non-positive count
    /// here always indicates a genuine over-release bug rather than a
    /// benign race, and is thrown instead of silently ignored.
    /// </summary>
    private void Decrement()
    {
        T* ptr;
        RustDestructor<T>? destructor;
        IRustHandleDependency[] dependencies;
        object[] pins;
        lock (_gate)
        {
            if (_refCount <= 0)
            {
                throw new InvalidOperationException(
                    $"{typeof(T).Name} native handle state released more times than it was retained.");
            }

            _refCount--;
            if (_refCount != 0)
            {
                return;
            }

            ptr = _ptr;
            _ptr = null;
            destructor = _destructor;
            dependencies = _dependencies;
            _dependencies = System.Array.Empty<IRustHandleDependency>();
            pins = _pins;
            _pins = System.Array.Empty<object>();
        }

        // Everything below runs outside the lock, on whichever single
        // thread's release happened to observe the count reach zero, and
        // strictly in this order: destructor, then unpin, then dependencies.
        try
        {
            if (ptr != null && destructor is not null)
            {
                destructor(ptr);
            }
        }
        finally
        {
            try
            {
                // Unpin only now, after the destructor above (if any) has
                // already run - never before, and never unconditionally on
                // every release call, since a call that merely decrements a
                // still-positive count must leave both the destructor and
                // these pins untouched for whichever later release actually
                // reaches zero.
                foreach (object pin in pins)
                {
                    (pin as IDisposable)?.Dispose();
                }
            }
            finally
            {
                foreach (IRustHandleDependency dependency in dependencies)
                {
                    dependency.Release();
                }
            }
        }
    }


    /// <summary>
    /// The token handed back by <see cref="Retain"/> for exactly one direct
    /// dependent. Its own <c>Release()</c> is one-shot-guarded independently
    /// of the owner and of every other token, so an accidental duplicate
    /// release of the SAME token can never double-decrement the shared
    /// count - see the type-level remarks.
    /// </summary>
    private sealed class DependencyToken : IRustHandleDependency
    {
        private readonly object _gate = new object();
        private RcRustHandleState<T>? _owner;

        internal DependencyToken(RcRustHandleState<T> owner)
        {
            _owner = owner;
        }

        public void Release()
        {
            RcRustHandleState<T>? owner;
            lock (_gate)
            {
                owner = _owner;
                _owner = null;
            }

            owner?.Decrement();
        }
    }
}

/// <summary>
/// A raw pointer plus the shared, reference-counted state
/// (<see cref="RcRustHandleState{T}"/>) that decides when it's actually safe
/// to run Rust's destructor on it. An owned handle carries the Rust
/// destructor; a borrowed handle carries none, so its release step never
/// frees memory Rust still owns - it only releases whatever dependencies it
/// retained. Used only by opaque types the generator classified as an actual
/// borrow source - see <see cref="RustHandle{T}"/> for the plain,
/// non-allocating lane every other opaque type uses instead.
/// </summary>
/// <remarks>
/// <see cref="IsNull"/> tracks whether <em>this specific handle instance</em>
/// has been released - it says nothing about whether the underlying native
/// resource has been physically destroyed, which instead depends on every
/// holder of the shared state (this handle's owner and every dependent)
/// having released their reference. See <see cref="RcRustHandleState{T}"/>.
/// </remarks>
internal readonly unsafe struct RcRustHandle<T> where T : unmanaged
{
    private readonly RcRustHandleState<T>? _state;

    private RcRustHandle(RcRustHandleState<T>? state)
    {
        _state = state;
    }

    /// <summary>The C# side owns the pointer, with no direct dependencies.</summary>
    internal static RcRustHandle<T> Owned(T* ptr, RustDestructor<T> destructor) =>
        Owned(ptr, destructor, System.Array.Empty<IRustHandleDependency>(), System.Array.Empty<object>());

    /// <summary>
    /// The C# side owns the pointer and this wrapper also borrows from one or
    /// more other wrappers (an "owned-borrowing" dependent) - each entry in
    /// <paramref name="dependencies"/> was already retained by the caller
    /// (see <see cref="Retain"/>) before this call.
    /// </summary>
    internal static RcRustHandle<T> Owned(T* ptr, RustDestructor<T> destructor, IRustHandleDependency[] dependencies) =>
        Owned(ptr, destructor, dependencies, System.Array.Empty<object>());

    /// <summary>
    /// The C# side owns the pointer and this wrapper pins one or more of its
    /// OWN input buffers (e.g. a <c>ReadOnlyMemory</c> parameter it borrows -
    /// see the <paramref name="pins"/> parameter on
    /// <see cref="RcRustHandleState{T}"/> for the ordering guarantee: these
    /// are only unpinned after this value's own Rust destructor actually
    /// runs, however long that is deferred by an outstanding dependent).
    /// </summary>
    internal static RcRustHandle<T> Owned(T* ptr, RustDestructor<T> destructor, object[] pins) =>
        Owned(ptr, destructor, System.Array.Empty<IRustHandleDependency>(), pins);

    /// <summary>
    /// The C# side owns the pointer, this wrapper borrows from one or more
    /// other wrappers, AND pins one or more of its own input buffers - the
    /// combination of both prior overloads.
    /// </summary>
    internal static RcRustHandle<T> Owned(T* ptr, RustDestructor<T> destructor, IRustHandleDependency[] dependencies, object[] pins) =>
        new RcRustHandle<T>(new RcRustHandleState<T>(ptr, destructor, dependencies, pins));

    /// <summary>Rust still owns the pointer, with no direct dependencies.</summary>
    internal static RcRustHandle<T> Borrowed(T* ptr) =>
        Borrowed(ptr, System.Array.Empty<IRustHandleDependency>());

    /// <summary>
    /// Rust still owns the pointer, but this view itself also borrows from
    /// one or more other wrappers. Releasing this handle never touches
    /// <paramref name="ptr"/> - it only releases the retained dependencies. A
    /// borrowed handle never carries its own pins: Rust still owns
    /// <paramref name="ptr"/>, so there is no destructor here to defer an
    /// unpin behind (see `gen::method::output_keep_alive_edges`: a borrowed
    /// return structurally never produces pins).
    /// </summary>
    internal static RcRustHandle<T> Borrowed(T* ptr, IRustHandleDependency[] dependencies) =>
        new RcRustHandle<T>(new RcRustHandleState<T>(ptr, null, dependencies, System.Array.Empty<object>()));

    internal T* Ptr => _state is null ? null : _state.Ptr;

    /// <summary>True once this specific handle instance has been released.</summary>
    internal bool IsNull => _state is null;

    /// <summary>
    /// Retains the shared native resource state for a new direct dependent,
    /// returning the dependency the dependent must release exactly once -
    /// from its own cleanup, after it has run its own Rust destructor (if
    /// any). Throws if this handle has already been released: a disposed
    /// wrapper has nothing left to lend a dependent.
    /// </summary>
    internal IRustHandleDependency Retain()
    {
        RcRustHandleState<T>? state = _state;
        if (state is null)
        {
            throw new ObjectDisposedException(typeof(T).Name);
        }

        return state.Retain();
    }

    /// <summary>
    /// Releases this handle's own ("owner") reference to the shared state.
    /// Idempotent at the state level (see
    /// <see cref="RcRustHandleState{T}.ReleaseOwner"/>): physical Rust
    /// destruction only happens once every reference - this one and every
    /// dependent's - has been released; see <see cref="RcRustHandleState{T}"/>.
    /// </summary>
    internal void Release()
    {
        _state?.ReleaseOwner();
    }
}