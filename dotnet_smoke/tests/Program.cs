// End-to-end smoke runner for the .NET Diplomat backend.
//
// Exercises the generated bindings against the real Rust cdylib:
//   * Opaque (Color) — Box<Self> wrapping, &self / &mut self methods,
//                       IDisposable.Dispose() actually calls *_destroy,
//                       post-Dispose access throws ObjectDisposedException.
//   * Struct  (Point2D) — by-value bridge via AsFFI/FromFFI, primitive
//                       returns, struct returns.
//   * Enum    (Status) — discriminant values match the Rust source.
//
// Asserts are hand-rolled (no xunit/etc) so the binary is dependency-free.

using System;
using DotnetSmoke;

namespace DotnetSmoke.Tests;

internal static class Program
{
    private static int passed;
    private static int failed;

    private static int Main()
    {
        Console.WriteLine("dotnet_smoke end-to-end tests\n");

        TestOpaqueColor();
        TestStructPoint2D();
        TestEnumStatus();
        TestDiplomatStr();
        TestBytesSlice();
        TestMutBytesSlice();
        TestResult();
        TestPropertiesAndBool();
        TestUnitResult();
        TestU32Slices();
        TestOptionalOpaqueInput();
        TestOpaqueErrorResult();
        TestOption();
        TestWrite();

        Console.WriteLine($"\n{passed} passed, {failed} failed");
        return failed == 0 ? 0 : 1;
    }

    // ─────────────────────────────────────────────────────────────────────
    // Opaque: Color
    // ─────────────────────────────────────────────────────────────────────

    private static void TestOpaqueColor()
    {
        Console.WriteLine("Color (opaque)");

        Test("Color.New() returns non-null wrapper", () =>
        {
            using var c = Color.New();
            // Reaching here means the static factory + wrapper ctor + raw
            // P/Invoke all worked. No assertion needed beyond no-throw.
        });

        Test("Brightness() defaults to 0", () =>
        {
            using var c = Color.New();
            AssertEqual<byte>(0, c.Brightness(), "initial brightness");
        });

        Test("SetBrightness round-trips through &mut self", () =>
        {
            using var c = Color.New();
            c.SetBrightness(128);
            AssertEqual<byte>(128, c.Brightness(), "after SetBrightness(128)");
            c.SetBrightness(255);
            AssertEqual<byte>(255, c.Brightness(), "after SetBrightness(255)");
        });

        Test("Dispose() can be called multiple times without crashing", () =>
        {
            var c = Color.New();
            c.Dispose();
            c.Dispose(); // second call must be a no-op, not a double-free
        });

        Test("Brightness() throws ObjectDisposedException after Dispose", () =>
        {
            var c = Color.New();
            c.Dispose();
            try
            {
                c.Brightness();
                throw new Exception("expected ObjectDisposedException, got none");
            }
            catch (ObjectDisposedException)
            {
                // expected
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Struct: Point2D
    // ─────────────────────────────────────────────────────────────────────

    private static void TestStructPoint2D()
    {
        Console.WriteLine("\nPoint2D (struct)");

        Test("New(3, 4) constructs with the right fields", () =>
        {
            var p = Point2D.New(3f, 4f);
            AssertEqual(3f, p.X, "X");
            AssertEqual(4f, p.Y, "Y");
        });

        Test("Magnitude() returns sqrt(x*x + y*y)", () =>
        {
            var p = Point2D.New(3f, 4f);
            AssertEqual(5f, p.Magnitude(), "magnitude of (3, 4)");
        });

        Test("Magnitude() on (0, 0) is 0", () =>
        {
            var p = Point2D.New(0f, 0f);
            AssertEqual(0f, p.Magnitude(), "magnitude of origin");
        });

        Test("Translated() returns a new struct with shifted fields", () =>
        {
            var p = Point2D.New(3f, 4f);
            var q = p.Translated(1f, -2f);
            AssertEqual(4f, q.X, "Translated.X");
            AssertEqual(2f, q.Y, "Translated.Y");
            // Original is untouched (value semantics)
            AssertEqual(3f, p.X, "original.X unchanged");
            AssertEqual(4f, p.Y, "original.Y unchanged");
        });

        Test("Bridge round-trip: FromFFI(AsFFI(p)) preserves the struct", () =>
        {
            var p = Point2D.New(7f, 11f);
            var roundTrip = Point2D.FromFFI(p.AsFFI());
            AssertEqual(7f, roundTrip.X, "round-trip X");
            AssertEqual(11f, roundTrip.Y, "round-trip Y");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Enum: Status
    // ─────────────────────────────────────────────────────────────────────

    private static void TestEnumStatus()
    {
        Console.WriteLine("\nStatus (enum)");

        Test("Status.Ok == 0", () =>
        {
            AssertEqual(0, (int)Status.Ok, "Status.Ok");
        });

        Test("Status.Err == 1", () =>
        {
            AssertEqual(1, (int)Status.Err, "Status.Err");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // &DiplomatStr (UTF-8 byte slice input)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestDiplomatStr()
    {
        Console.WriteLine("\n&DiplomatStr (UTF-8 byte slice)");

        // ---- NameLength: bytes count, not char count ------------------

        Test("NameLength(\"\") == 0", () =>
        {
            AssertEqual<uint>(0, Color.NameLength(""), "empty string byte count");
        });

        Test("NameLength(\"hello\") == 5 (pure ASCII)", () =>
        {
            AssertEqual<uint>(5, Color.NameLength("hello"), "ASCII byte count");
        });

        Test("NameLength(\"héllo\") == 6 (é is 2 UTF-8 bytes)", () =>
        {
            // 5 chars but 6 UTF-8 bytes: h(1) é(2) l(1) l(1) o(1) = 6
            AssertEqual<uint>(6, Color.NameLength("héllo"), "mixed-encoding byte count");
        });

        Test("NameLength(\"中文\") == 6 (each CJK char is 3 UTF-8 bytes)", () =>
        {
            // 2 chars but 6 UTF-8 bytes: 中(3) 文(3) = 6
            AssertEqual<uint>(6, Color.NameLength("中文"), "CJK byte count");
        });

        // ---- NameFirstByte: content crosses (not just length) ---------

        Test("NameFirstByte(\"\") == 0 (no bytes)", () =>
        {
            AssertEqual<byte>(0, Color.NameFirstByte(""), "empty first byte");
        });

        Test("NameFirstByte(\"hello\") == 0x68 ('h')", () =>
        {
            AssertEqual<byte>(0x68, Color.NameFirstByte("hello"), "ASCII first byte");
        });

        Test("NameFirstByte(\"héllo\") == 0x68 ('h')", () =>
        {
            // First char is still ASCII 'h' = 0x68, even though 'é' follows.
            AssertEqual<byte>(0x68, Color.NameFirstByte("héllo"), "first byte before multi-byte char");
        });

        Test("NameFirstByte(\"中文\") == 0xE4 (first UTF-8 byte of '中')", () =>
        {
            // '中' (U+4E2D) encodes as 0xE4 0xB8 0xAD; first byte is 0xE4.
            AssertEqual<byte>(0xE4, Color.NameFirstByte("中文"), "first byte of CJK char");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // &[u8] (immutable byte slice input)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestBytesSlice()
    {
        Console.WriteLine("\n&[u8] (immutable byte slice)");

        Test("Checksum(empty) == 0", () =>
        {
            AssertEqual<uint>(0, Color.Checksum(new byte[0]), "empty checksum");
        });

        Test("Checksum([1, 2, 3, 4]) == 10", () =>
        {
            AssertEqual<uint>(10, Color.Checksum(new byte[] { 1, 2, 3, 4 }), "sum");
        });

        Test("Checksum([0xFF, 0xFF]) == 510 (widens to u32, no overflow)", () =>
        {
            // Each byte widens to u32 in Rust, so 255 + 255 = 510 (not 254 truncated).
            AssertEqual<uint>(510, Color.Checksum(new byte[] { 0xFF, 0xFF }), "wide sum");
        });

        Test("Checksum(100 bytes of 0x01) == 100 (length crosses for larger arrays)", () =>
        {
            var bytes = new byte[100];
            for (int i = 0; i < 100; i++) bytes[i] = 1;
            AssertEqual<uint>(100, Color.Checksum(bytes), "100-byte sum");
        });

        Test("Checksum([0, 1, 2, ..., 9]) == 45 (positional reads all bytes correctly)", () =>
        {
            // Sum of 0..9 = 45. Confirms Rust isn't just reading the first byte.
            var bytes = new byte[10];
            for (byte i = 0; i < 10; i++) bytes[i] = i;
            AssertEqual<uint>(45, Color.Checksum(bytes), "positional sum");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // &mut [u8] (mutable byte slice — IronRDP unblock)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestMutBytesSlice()
    {
        Console.WriteLine("\n&mut [u8] (mutable byte slice — IronRDP unblock)");

        Test("FillBuffer writes brightness to every byte", () =>
        {
            using var c = Color.New();
            c.SetBrightness(128);

            var buf = new byte[4];
            c.FillBuffer(buf);

            AssertEqual<byte>(128, buf[0], "buf[0]");
            AssertEqual<byte>(128, buf[1], "buf[1]");
            AssertEqual<byte>(128, buf[2], "buf[2]");
            AssertEqual<byte>(128, buf[3], "buf[3]");
        });

        Test("FillBuffer overwrites pre-existing contents (writes propagate back)", () =>
        {
            using var c = Color.New();
            c.SetBrightness(0xAB);

            // Pre-fill with non-zero garbage; Rust must overwrite every byte.
            var buf = new byte[3] { 0x11, 0x22, 0x33 };
            c.FillBuffer(buf);

            AssertEqual<byte>(0xAB, buf[0], "buf[0] overwritten");
            AssertEqual<byte>(0xAB, buf[1], "buf[1] overwritten");
            AssertEqual<byte>(0xAB, buf[2], "buf[2] overwritten");
        });

        Test("FillBuffer reflects brightness mutation between calls", () =>
        {
            using var c = Color.New();
            var buf = new byte[2];

            c.SetBrightness(0xAB);
            c.FillBuffer(buf);
            AssertEqual<byte>(0xAB, buf[0], "first fill");
            AssertEqual<byte>(0xAB, buf[1], "first fill [1]");

            c.SetBrightness(0xCD);
            c.FillBuffer(buf);
            AssertEqual<byte>(0xCD, buf[0], "second fill overwrites");
            AssertEqual<byte>(0xCD, buf[1], "second fill overwrites [1]");
        });

        Test("FillBuffer on empty buffer doesn't crash", () =>
        {
            using var c = Color.New();
            c.SetBrightness(128);
            c.FillBuffer(new byte[0]);
            // success = no crash, no out-of-bounds write
        });

        Test("FillBuffer on disposed Color throws ObjectDisposedException", () =>
        {
            var c = Color.New();
            c.Dispose();
            try
            {
                c.FillBuffer(new byte[4]);
                throw new Exception("expected ObjectDisposedException, got none");
            }
            catch (ObjectDisposedException)
            {
                // expected
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Result<T, E> (errors-as-values → exceptions)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestResult()
    {
        Console.WriteLine("\nResult<T, E> (errors-as-values → exceptions)");

        // ---- Ok path: parse known names, get a Color back ----

        Test("Parse(\"red\") returns a Color with brightness 255", () =>
        {
            using var c = Color.Parse("red");
            AssertEqual<byte>(255, c.Brightness(), "red brightness");
        });

        Test("Parse(\"black\") returns a Color with brightness 0", () =>
        {
            using var c = Color.Parse("black");
            AssertEqual<byte>(0, c.Brightness(), "black brightness");
        });

        Test("Parsed Color participates in IDisposable lifecycle", () =>
        {
            var c = Color.Parse("red");
            c.Dispose();
            try
            {
                c.Brightness();
                throw new Exception("expected ObjectDisposedException, got none");
            }
            catch (ObjectDisposedException)
            {
                // expected — Ok wraps the raw pointer in a real IDisposable wrapper
            }
        });

        // ---- Err path: empty input → BadFormat ----

        Test("Parse(\"\") throws ColorException(BadFormat)", () =>
        {
            try
            {
                Color.Parse("");
                throw new Exception("expected ColorException, got none");
            }
            catch (ColorException ex)
            {
                AssertEqual(ColorError.BadFormat, ex.Inner, "BadFormat variant");
            }
        });

        // ---- Err path: unknown names → Unknown ----

        Test("Parse(\"rainbow\") throws ColorException(Unknown)", () =>
        {
            try
            {
                Color.Parse("rainbow");
                throw new Exception("expected ColorException, got none");
            }
            catch (ColorException ex)
            {
                AssertEqual(ColorError.Unknown, ex.Inner, "Unknown variant");
            }
        });

        Test("Parse(\"héllo\") throws Unknown (multi-byte input handled)", () =>
        {
            // Cross-check that slice scaffolding (Encoding.UTF8 + fixed) composes
            // correctly with Result returns. Same as &DiplomatStr tests, just
            // verifying nothing breaks when both features combine in one method.
            try
            {
                Color.Parse("héllo");
                throw new Exception("expected ColorException, got none");
            }
            catch (ColorException ex)
            {
                AssertEqual(ColorError.Unknown, ex.Inner, "Unknown for multibyte");
            }
        });

        // ---- Cross-validate: discriminant carries the variant value ----

        Test("Different inputs produce different error variants", () =>
        {
            ColorError empty;
            ColorError unknown;
            try { Color.Parse(""); empty = ColorError.BadFormat; throw new Exception("no throw"); }
            catch (ColorException ex) { empty = ex.Inner; }
            try { Color.Parse("xyz"); unknown = ColorError.Unknown; throw new Exception("no throw"); }
            catch (ColorException ex) { unknown = ex.Inner; }

            AssertEqual(ColorError.BadFormat, empty, "empty → BadFormat");
            AssertEqual(ColorError.Unknown, unknown, "xyz → Unknown");
            // Proof the wire carries the variant, not just "any error".
            if (empty == unknown)
            {
                throw new Exception("BadFormat and Unknown collapsed to same value — discriminant lost");
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Properties + bool marshaling
    // ─────────────────────────────────────────────────────────────────────

    private static void TestPropertiesAndBool()
    {
        Console.WriteLine("\nProperties + bool marshaling");

        Test("get_/set_ prefixes expose Level property", () =>
        {
            using var c = Color.New();
            c.Level = 77;
            AssertEqual<byte>(77, c.Level, "Level property");
            AssertEqual<byte>(77, c.Brightness(), "property setter updates Rust state");
        });

        Test("Bool return is marshaled as 1-byte C bool", () =>
        {
            using var c = Color.New();
            c.SetBrightness(127);
            AssertEqual(false, c.IsBright(), "127 is not bright");
            c.SetBrightness(128);
            AssertEqual(true, c.IsBright(), "128 is bright");
        });

        Test("Bool input is marshaled as 1-byte C bool", () =>
        {
            AssertEqual<byte>(1, Color.BoolToByte(true), "true");
            AssertEqual<byte>(0, Color.BoolToByte(false), "false");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Result<(), E>
    // ─────────────────────────────────────────────────────────────────────

    private static void TestUnitResult()
    {
        Console.WriteLine("\nResult<(), E> (unit success)");

        Test("RequireKnown(\"red\") returns without throwing", () =>
        {
            Color.RequireKnown("red");
        });

        Test("RequireKnown(\"\") throws ColorException(BadFormat)", () =>
        {
            try
            {
                Color.RequireKnown("");
                throw new Exception("expected ColorException, got none");
            }
            catch (ColorException ex)
            {
                AssertEqual(ColorError.BadFormat, ex.Inner, "BadFormat variant");
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // &[u32] and &mut [u32]
    // ─────────────────────────────────────────────────────────────────────

    private static void TestU32Slices()
    {
        Console.WriteLine("\n&[u32] and &mut [u32]");

        Test("ChecksumU32([1, 2, 3]) == 6", () =>
        {
            AssertEqual<uint>(6, Color.ChecksumU32(new uint[] { 1, 2, 3 }), "u32 checksum");
        });

        Test("FillU32 writes brightness to every uint", () =>
        {
            using var c = Color.New();
            c.SetBrightness(9);
            var values = new uint[3];
            c.FillU32(values);
            AssertEqual<uint>(9, values[0], "values[0]");
            AssertEqual<uint>(9, values[1], "values[1]");
            AssertEqual<uint>(9, values[2], "values[2]");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Optional opaque input
    // ─────────────────────────────────────────────────────────────────────

    private static void TestOptionalOpaqueInput()
    {
        Console.WriteLine("\nOptional opaque input");

        Test("MaybeBrightness(null) returns 0", () =>
        {
            AssertEqual<byte>(0, Color.MaybeBrightness(null), "null color");
        });

        Test("MaybeBrightness(Color) reads the pointee", () =>
        {
            using var c = Color.New();
            c.SetBrightness(55);
            AssertEqual<byte>(55, Color.MaybeBrightness(c), "present color");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Result<T, opaque E>
    // ─────────────────────────────────────────────────────────────────────

    private static void TestOpaqueErrorResult()
    {
        Console.WriteLine("\nResult<T, opaque E>");

        Test("ParseWithOpaqueError(\"red\") returns Color", () =>
        {
            using var c = Color.ParseWithOpaqueError("red");
            AssertEqual<byte>(255, c.Brightness(), "red brightness");
        });

        Test("ParseWithOpaqueError(\"\") throws ParseException with writer message", () =>
        {
            try
            {
                Color.ParseWithOpaqueError("");
                throw new Exception("expected ParseException, got none");
            }
            catch (ParseException ex)
            {
                if (!ex.Message.Contains("empty color name"))
                {
                    throw new Exception($"wrong message: {ex.Message}");
                }
            }
        });

        Test("FindWithOpaqueError can return Some, None, or Err", () =>
        {
            using var hit = Color.FindWithOpaqueError("red");
            if (hit is null) throw new Exception("expected Some");
            AssertEqual<byte>(255, hit.Brightness(), "red brightness");

            var miss = Color.FindWithOpaqueError("rainbow");
            if (miss is not null) throw new Exception("expected None");

            try
            {
                Color.FindWithOpaqueError("");
                throw new Exception("expected ParseException, got none");
            }
            catch (ParseException ex)
            {
                if (!ex.Message.Contains("empty color name"))
                {
                    throw new Exception($"wrong message: {ex.Message}");
                }
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Option<T> (absence → nullable types, no exceptions)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestOption()
    {
        Console.WriteLine("\nOption<T> (absence → nullable types)");

        // ---- Path A: Option<Box<T>> → Color? (pointer-nullable) ----

        Test("FindByName(\"red\") returns non-null Color with brightness 255", () =>
        {
            using var c = Color.FindByName("red");
            if (c is null) throw new Exception("expected Some, got null");
            AssertEqual<byte>(255, c.Brightness(), "red brightness");
        });

        Test("FindByName(\"black\") returns non-null Color with brightness 0", () =>
        {
            using var c = Color.FindByName("black");
            if (c is null) throw new Exception("expected Some, got null");
            AssertEqual<byte>(0, c.Brightness(), "black brightness");
        });

        Test("FindByName(\"rainbow\") returns null (None)", () =>
        {
            var c = Color.FindByName("rainbow");
            if (c is not null) throw new Exception($"expected null, got Color with brightness {c.Brightness()}");
        });

        Test("FindByName(\"\") returns null (None on empty input)", () =>
        {
            var c = Color.FindByName("");
            if (c is not null) throw new Exception("expected null for empty input");
        });

        // ---- Path B: Option<u8> → byte? (tagged DiplomatOption struct) ----

        Test("BrightnessOf(\"red\") returns 255", () =>
        {
            byte? b = Color.BrightnessOf("red");
            if (!b.HasValue) throw new Exception("expected Some");
            AssertEqual<byte>(255, b.Value, "red");
        });

        Test("BrightnessOf(\"black\") returns 0 (Some(0), not None)", () =>
        {
            // The semantic distinction matters: 0 is a real value, not None.
            // If the binding confused them, this would fail.
            byte? b = Color.BrightnessOf("black");
            if (!b.HasValue) throw new Exception("expected Some(0), not None");
            AssertEqual<byte>(0, b.Value, "black");
        });

        Test("BrightnessOf(\"gray\") returns 128", () =>
        {
            byte? b = Color.BrightnessOf("gray");
            if (!b.HasValue) throw new Exception("expected Some");
            AssertEqual<byte>(128, b.Value, "gray");
        });

        Test("BrightnessOf(\"rainbow\") returns null (None)", () =>
        {
            byte? b = Color.BrightnessOf("rainbow");
            if (b.HasValue) throw new Exception($"expected null, got Some({b.Value})");
        });

        // ---- Cross-validation: discriminant carries through both paths ----

        Test("Different inputs produce distinguishable Option results (Path A)", () =>
        {
            using var hit  = Color.FindByName("red");
            var miss = Color.FindByName("rainbow");
            if (hit is null || miss is not null)
                throw new Exception("Some/None discriminant lost on Path A");
        });

        Test("Different inputs produce distinguishable Option results (Path B)", () =>
        {
            byte? hit  = Color.BrightnessOf("gray");
            byte? miss = Color.BrightnessOf("rainbow");
            if (!hit.HasValue || miss.HasValue)
                throw new Exception("Some/None discriminant lost on Path B");
            AssertEqual<byte>(128, hit.Value, "gray value");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // DiplomatWrite (writer pattern → C# string return)
    // ─────────────────────────────────────────────────────────────────────

    private static void TestWrite()
    {
        Console.WriteLine("\nDiplomatWrite (writer pattern → string return)");

        // ---- Basic: short output fits in initial 64-byte buffer ----

        Test("Describe() returns formatted string with brightness state", () =>
        {
            using var c = Color.New();
            c.SetBrightness(128);
            AssertEqual<string>("Color(brightness=128)", c.Describe(), "default brightness");
        });

        Test("Describe() reflects brightness=0 (no Some(0)-vs-None confusion)", () =>
        {
            using var c = Color.New();
            c.SetBrightness(0);
            AssertEqual<string>("Color(brightness=0)", c.Describe(), "min");
        });

        Test("Describe() reflects brightness=255", () =>
        {
            using var c = Color.New();
            c.SetBrightness(255);
            AssertEqual<string>("Color(brightness=255)", c.Describe(), "max");
        });

        Test("Describe() works on disposed Color throws ObjectDisposedException", () =>
        {
            var c = Color.New();
            c.Dispose();
            try
            {
                c.Describe();
                throw new Exception("expected ObjectDisposedException, got none");
            }
            catch (ObjectDisposedException)
            {
                // expected — disposed check fires before allocating writer
            }
        });

        // ---- Buffer growth: long output exceeds 64-byte initial capacity ----
        // Forces the Grow callback into C# — the load-bearing path of the
        // DiplomatWriteable design.

        Test("LongDescription() handles output longer than initial 64-byte buffer (grow callback fires)", () =>
        {
            using var c = Color.New();
            c.SetBrightness(42);
            string result = c.LongDescription();
            // Result should be ~100 bytes — exceeds the initial 64-byte
            // capacity. If Grow callback didn't fire correctly, this would
            // truncate, crash, or return garbage.
            if (result.Length <= 64) throw new Exception($"expected output > 64 chars, got {result.Length}");
            if (!result.Contains("brightness=42")) throw new Exception("should reflect current brightness");
            if (!result.Contains("padding=")) throw new Exception("should include the long padding section");
        });

        Test("LongDescription() reflects current state across multiple grow calls", () =>
        {
            using var c = Color.New();

            c.SetBrightness(1);
            string r1 = c.LongDescription();
            if (!r1.Contains("brightness=1")) throw new Exception("first call should contain brightness=1");

            c.SetBrightness(99);
            string r2 = c.LongDescription();
            if (!r2.Contains("brightness=99")) throw new Exception("second call should contain brightness=99");

            // Different inputs produce different outputs — confirms each
            // call gets a fresh writer with fresh state, no stale buffer.
            if (r1 == r2) throw new Exception("expected different outputs across mutations");
        });
    }

    // ─────────────────────────────────────────────────────────────────────
    // Test harness
    // ─────────────────────────────────────────────────────────────────────

    private static void Test(string name, Action action)
    {
        try
        {
            action();
            Console.WriteLine($"  PASS  {name}");
            passed++;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"  FAIL  {name}");
            Console.WriteLine($"        {ex.GetType().Name}: {ex.Message}");
            failed++;
        }
    }

    private static void AssertEqual<T>(T expected, T actual, string what)
    {
        if (!Equals(expected, actual))
        {
            throw new Exception($"{what}: expected `{expected}`, got `{actual}`");
        }
    }
}
