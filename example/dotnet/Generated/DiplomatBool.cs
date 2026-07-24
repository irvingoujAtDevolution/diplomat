using System.Runtime.InteropServices;

namespace Somelib.Diplomat;

// A blittable, one-byte stand-in for `bool`, used for the Result/Option
// discriminant that comes back inside a struct returned *by value* from a
// P/Invoke.
//
// Why this type exists:
//   The .NET Framework CLR (net48 and earlier) refuses to marshal a struct
//   returned by value when any field is non-blittable, and `bool` is
//   non-blittable — its native width is ambiguous, so the marshaller must
//   convert it. The failure is a runtime `MarshalDirectiveException` ("Method's
//   type signature is not PInvoke compatible") thrown on the first call, with
//   no compile-time warning. Modern .NET (Core / 5+) tolerates it, but the
//   generated bindings ship as netstandard2.0 and must also load on .NET
//   Framework 4.x, so a plain `bool` field breaks those consumers.
//
// Why a single byte is correct on every target:
//   The Rust side is `#[repr(C)] struct DiplomatResult { value: union, is_ok:
//   bool }`. Rust guarantees `bool` is exactly one byte, holding 0 (false) or 1
//   (true), on every platform. A one-byte C# field at the same offset therefore
//   matches the C ABI layout on all architectures (x86, x64, ARM, ARM64,
//   wasm32) and operating systems (Windows, Linux, macOS, iOS, Android) — both
//   `#[repr(C)]` and `[StructLayout(Sequential)]` follow the target's C ABI for
//   size, alignment, and padding. Reading any non-zero byte as `true` also
//   matches Rust's bool semantics.
[StructLayout(LayoutKind.Sequential)]
internal readonly struct DiplomatBool
{
    private readonly byte _value;

    private DiplomatBool(byte value) => _value = value;

    // Converts both ways so the idiomatic layer stays `bool`-only: a `bool`
    // argument becomes a `DiplomatBool` on the way into the raw layer, and a
    // `DiplomatBool` field/return reads back as `bool`. Rust and the public C#
    // API never see this type.
    public static implicit operator DiplomatBool(bool value) => new DiplomatBool(value ? (byte)1 : (byte)0);
    public static implicit operator bool(DiplomatBool value) => value._value != 0;
}