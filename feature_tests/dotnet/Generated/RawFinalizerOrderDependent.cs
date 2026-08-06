using System;
using System.Runtime.InteropServices;
using Somelib;
using Somelib.Diplomat;

namespace Somelib.Raw;

[StructLayout(LayoutKind.Sequential)]
internal partial struct FinalizerOrderDependent
{

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderDependent_reads_source", CallingConvention = CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.U1)]
    internal static unsafe extern bool ReadsSource(FinalizerOrderDependent* handle);

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderDependent_destroy", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern void Destroy(FinalizerOrderDependent* handle);
}