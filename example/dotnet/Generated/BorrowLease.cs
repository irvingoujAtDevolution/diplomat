using System;
using System.Threading;

namespace Somelib.Diplomat;

#nullable enable

internal enum BorrowKind
{
    Shared,
    Exclusive,
}

internal interface IBorrowLease
{
    IDisposable Transfer();
    IDisposable TransferVersioned();
}

internal interface IVersionedBorrow
{
    IDisposable Acquire();
}

internal sealed unsafe class BorrowLease<T> : IBorrowLease, IDisposable where T : unmanaged
{
    private RustHandle<T>? _owner;
    private readonly BorrowKind _kind;
    private IDisposable[] _dependencies;

    internal BorrowLease(RustHandle<T> owner, BorrowKind kind, IDisposable[] dependencies)
    {
        _owner = owner;
        _kind = kind;
        _dependencies = dependencies;
        Ptr = owner.Ptr;
    }

    internal T* Ptr { get; }

    internal IDisposable Transfer()
    {
        RustHandle<T>? owner = Interlocked.Exchange(ref _owner, null);
        if (owner is null)
        {
            throw new ObjectDisposedException(nameof(BorrowLease<T>));
        }

        return new BorrowToken(owner, _kind, TakeDependencies());
    }

    IDisposable IBorrowLease.Transfer() => Transfer();

    internal IDisposable TransferVersioned()
    {
        RustHandle<T>? owner = Interlocked.Exchange(ref _owner, null);
        if (owner is null)
        {
            throw new ObjectDisposedException(nameof(BorrowLease<T>));
        }

        long version = owner.ReleaseBorrowForVersion(_kind);
        VersionedBorrowToken token = new VersionedBorrowToken(owner, version);
        try
        {
            ReleaseDependencies(TakeDependencies());
            return token;
        }
        catch
        {
            token.Dispose();
            throw;
        }
    }

    IDisposable IBorrowLease.TransferVersioned() => TransferVersioned();

    public void Dispose()
    {
        RustHandle<T>? owner = Interlocked.Exchange(ref _owner, null);
        if (owner is null)
        {
            return;
        }

        try
        {
            ReleaseDependencies(TakeDependencies());
        }
        finally
        {
            owner.ReleaseBorrow(_kind);
        }
    }

    private IDisposable[] TakeDependencies()
    {
        IDisposable[] dependencies = _dependencies;
        _dependencies = System.Array.Empty<IDisposable>();
        return dependencies;
    }

    private static void ReleaseDependencies(IDisposable[] dependencies)
    {
        for (int i = dependencies.Length - 1; i >= 0; i--)
        {
            dependencies[i].Dispose();
        }
    }

    private sealed class BorrowToken : IDisposable
    {
        private RustHandle<T>? _owner;
        private readonly BorrowKind _kind;
        private IDisposable[] _dependencies;

        internal BorrowToken(RustHandle<T> owner, BorrowKind kind, IDisposable[] dependencies)
        {
            _owner = owner;
            _kind = kind;
            _dependencies = dependencies;
        }

        public void Dispose()
        {
            RustHandle<T>? owner = Interlocked.Exchange(ref _owner, null);
            if (owner is null)
            {
                return;
            }

            try
            {
                IDisposable[] dependencies = _dependencies;
                _dependencies = System.Array.Empty<IDisposable>();
                ReleaseDependencies(dependencies);
            }
            finally
            {
                owner.ReleaseBorrow(_kind);
            }
        }
    }

    private sealed class VersionedBorrowToken : IVersionedBorrow, IDisposable
    {
        private RustHandle<T>? _owner;
        private readonly long _version;

        internal VersionedBorrowToken(RustHandle<T> owner, long version)
        {
            _owner = owner;
            _version = version;
        }

        IDisposable IVersionedBorrow.Acquire()
        {
            RustHandle<T>? owner = Volatile.Read(ref _owner);
            if (owner is null)
            {
                throw new ObjectDisposedException(nameof(VersionedBorrowToken));
            }

            if (owner.IsScopeEnded || owner.Version != _version)
            {
                throw Invalidated();
            }

            BorrowLease<T> lease = owner.BorrowShared();
            if (!owner.IsScopeEnded && owner.Version == _version)
            {
                return lease;
            }

            lease.Dispose();
            throw Invalidated();
        }

        public void Dispose()
        {
            RustHandle<T>? owner = Interlocked.Exchange(ref _owner, null);
            owner?.ReleaseClaim();
        }

        private static InvalidOperationException Invalidated() =>
            new InvalidOperationException(
                "This borrowed view was invalidated by mutation of its source."
            );
    }
}