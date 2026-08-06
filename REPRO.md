# .NET finalizer ordering repro

This standalone harness tests whether a strong managed `_edges` reference orders
two native-style finalizers when neither wrapper implements `IDisposable`.

It compares:

1. an edge-only dependent that strongly references its source; and
2. the same edge plus a `SafeHandle.DangerousAddRef` held until the dependent
   release completes.

Run:

```powershell
cd experiments\dotnet-finalizer-order
dotnet run -c Release
```

Environment used:

```text
.NET 10.0.10
Windows x64
```

Observed in five consecutive runs:

```text
Creating 100,000 finalizable pairs per model...
edge-only: source=100,000, dependent=100,000, source-first=1
refcount:  source=100,000, dependent=100,000, source-first=0
```

The harness exits with code 2 if edge-only ordering does not reproduce and code
3 if the refcounted model ever releases the source first.

No wrapper in either model exposes or calls `Dispose`. Both dependents retain
the same strong `_edges` reference. The only difference is the source
`SafeHandle` reference held by the refcounted dependent.
