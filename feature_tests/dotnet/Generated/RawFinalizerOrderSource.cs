using System;
using System.Runtime.InteropServices;
using Somelib;
using Somelib.Diplomat;

namespace Somelib.Raw;

[StructLayout(LayoutKind.Sequential)]
internal partial struct FinalizerOrderSource
{

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_create", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern FinalizerOrderSource* Create();

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_make_dependent", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern FinalizerOrderDependent* MakeDependent(FinalizerOrderSource* handle);

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_reset_probe", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern void ResetProbe();

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_source_drops", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern ulong SourceDrops();

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_dependent_drops", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern ulong DependentDrops();

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_bad_order_drops", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern ulong BadOrderDrops();

    [DllImport(DiplomatNativeLib.Name, EntryPoint = "FinalizerOrderSource_destroy", CallingConvention = CallingConvention.Cdecl)]
    internal static unsafe extern void Destroy(FinalizerOrderSource* handle);
}