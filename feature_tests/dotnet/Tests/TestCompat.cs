using System.Runtime.CompilerServices;

namespace Somelib.FeatureTests;

// Small shims for BCL bits that exist on modern .NET but not on .NET Framework,
// so the same test sources compile on both net8.0 and net48.
internal static class TestCompat
{
    // net48 has no MethodImplOptions.AggressiveOptimization (value 512). It's a
    // JIT optimization hint the older runtime simply ignores, so passing the raw
    // value there is harmless; modern .NET gets the real named flag.
#if NET48
    public const MethodImplOptions AggressiveOptimization = (MethodImplOptions)512;
#else
    public const MethodImplOptions AggressiveOptimization = MethodImplOptions.AggressiveOptimization;
#endif
}
