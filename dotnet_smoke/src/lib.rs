//! Tiny smoke crate to drive the `.NET` Diplomat backend during development.
//!
//! Iteration loop (from `D:/diplomat`):
//!     cargo run -p diplomat-tool -- dotnet dotnet_smoke/out \
//!         --entry dotnet_smoke/src/lib.rs \
//!         --config-file dotnet_smoke/config.toml
//!
//! Or, with auto-rerun on change:
//!     cargo watch -w tool/src/dotnet -w tool/templates/dotnet -w dotnet_smoke/src \
//!         -x "run -p diplomat-tool -- dotnet dotnet_smoke/out \
//!             --entry dotnet_smoke/src/lib.rs --config-file dotnet_smoke/config.toml"
//!
//! Generated `.cs` files land in `dotnet_smoke/out/` (gitignored).
//!
//! Stages — uncomment as the backend grows:
//!   * Stage 1 (current): a single enum. Exercises `gen_enum` + the enum template.
//!   * Stage 2: an opaque type. Exercises `gen_opaque`, IDisposable shape.
//!   * Stage 3: a method with primitives. Exercises the converter for primitives.
//!   * Stage 4: `&DiplomatStr` input. Exercises slice converter.
//!   * Stage 5: `Result<T, E>` return. Exercises DiplomatResult converter.

#[diplomat::bridge]
pub mod ffi {
    /// A tiny enum that exists purely to drive the .NET backend's
    /// enum codegen path.
    pub enum Status {
        Ok = 0,
        Err = 1,
    }


    // -- Stage 2: opaque type ---------------------------------------------
    //
    // Opaque with state + the three method shapes you'll see everywhere:
    //   - `new()`        : static, returns `Box<Self>`
    //   - `brightness()` : `&self`,    primitive return
    //   - `set_brightness()`: `&mut self`, primitive param, no return
    #[diplomat::opaque_mut]
    pub struct Color {
        brightness: u8,
    }

    impl Color {
        pub fn new() -> Box<Color> {
            Box::new(Color { brightness: 0 })
        }

        pub fn brightness(&self) -> u8 {
            self.brightness
        }

        pub fn set_brightness(&mut self, value: u8) {
            self.brightness = value;
        }

        pub fn get_level(&self) -> u8 {
            self.brightness
        }

        pub fn set_level(&mut self, value: u8) {
            self.brightness = value;
        }

        pub fn is_bright(&self) -> bool {
            self.brightness > 127
        }

        pub fn bool_to_byte(value: bool) -> u8 {
            if value { 1 } else { 0 }
        }

        pub fn maybe_brightness(color: Option<&Color>) -> u8 {
            color.map(|c| c.brightness).unwrap_or(0)
        }
    }

    // -- Stage 3: struct with primitives ----------------------------------
    //
    // A by-value Diplomat struct: public primitive fields, static
    // constructor, and methods that consume `self`. Exercises `gen_struct`
    // and the `f32` arm of `primitives_to_dotnet_type`.
    pub struct Point2D {
        pub x: f32,
        pub y: f32,
    }

    impl Point2D {
        pub fn new(x: f32, y: f32) -> Point2D {
            Point2D { x, y }
        }


        pub fn magnitude(self) -> f32 {
            (self.x * self.x + self.y * self.y).sqrt()
        }

        pub fn translated(self, dx: f32, dy: f32) -> Point2D {
            Point2D {
                x: self.x + dx,
                y: self.y + dy,
            }
        }
    }

    // -- Stage 4: &DiplomatStr input --------------------------------------
    //
    // Two static methods on `Color` exercise the `&DiplomatStr` lowering
    // end-to-end. They take a UTF-8 byte slice as input and return
    // primitives, so the test only depends on slice marshaling (no
    // `Option` / `Result` / opaque return wiring needed yet).
    //
    // Cross-validate the binding: `name_length` proves the length crosses
    // correctly; `name_first_byte` proves the *bytes* arrive too. If C#
    // passed `(null, 5)` instead of `("héllo", 5)`, `name_length` would
    // still return 5 but `name_first_byte` would not return 0x68 (`'h'`).
    impl Color {
        /// Bytes the slice carries (UTF-8 byte count, not char count).
        /// `"héllo"` → 6 because `é` is two UTF-8 bytes.
        pub fn name_length(name: &DiplomatStr) -> u32 {
            name.len() as u32
        }

        /// First byte of the slice, or 0 if empty. Verifies the *content*
        /// crosses, not just the length.
        pub fn name_first_byte(name: &DiplomatStr) -> u8 {
            name.first().copied().unwrap_or(0)
        }
    }

    // -- Stage 4b: &[u8] and &mut [u8] (the IronRDP shape) ---------------
    //
    // `&[u8]` is the same lowering as `&DiplomatStr` minus the UTF-8
    // encoding step — caller already has bytes. `&mut [u8]` is the real
    // prize: Rust writes through caller-owned memory, zero copies,
    // writes propagate back to the caller after the call returns. This
    // is what IronRDP needs for frame decoding and what uniffi can't do.
    //
    // `checksum` proves read-only bytes cross correctly (Rust sums them).
    // `fill_buffer` proves &mut: the C# caller passes a zeroed buffer,
    // Rust writes brightness to every byte, the caller observes the
    // writes via their Span<byte>.
    impl Color {
        /// Sum of the bytes, widened to u32. Proves Rust reads each byte
        /// at its correct position.
        pub fn checksum(bytes: &[u8]) -> u32 {
            bytes.iter().map(|&b| b as u32).sum()
        }

        /// Fill the caller's buffer with `self.brightness`. The buffer
        /// is caller-owned memory; Rust writes through the &mut borrow
        /// and the caller sees the bytes after the call returns.
        pub fn fill_buffer(&self, buf: &mut [u8]) {
            for b in buf.iter_mut() {
                *b = self.brightness;
            }
        }

        pub fn checksum_u32(values: &[u32]) -> u32 {
            values.iter().copied().sum()
        }

        pub fn fill_u32(&self, values: &mut [u32]) {
            for value in values.iter_mut() {
                *value = self.brightness as u32;
            }
        }
    }

    // -- Stage 5: Result<T, E> return -------------------------------------
    //
    // Rust's errors-as-values meets C#'s exceptions. The Rust source uses
    // `Result<Box<Color>, ColorError>` natively; the macro lowers that to
    // `DiplomatResult<*mut Color, ColorError>` at the FFI boundary (a
    // tagged union: 8-byte payload + 1-byte is_ok + 7 bytes padding =
    // 16 bytes on x64). The C# binding will check the tag and either
    // wrap+return the pointer or throw a ColorException carrying the
    // error variant.
    //
    // `ColorError` has two variants so we can verify the binding actually
    // carries the variant value across, not just "any throw":
    //   - `BadFormat` for empty input
    //   - `Unknown` for any other unrecognized name

    pub enum ColorError {
        Unknown   = 0,
        BadFormat = 1,
    }

    impl Color {
        /// Look up a named color. Recognizes a small fixed set; returns
        /// `BadFormat` for empty input and `Unknown` for anything else.
        pub fn parse(s: &DiplomatStr) -> Result<Box<Color>, ColorError> {
            match s {
                b""      => Err(ColorError::BadFormat),
                b"red"   => Ok(Box::new(Color { brightness: 255 })),
                b"black" => Ok(Box::new(Color { brightness: 0 })),
                _        => Err(ColorError::Unknown),
            }
        }

        pub fn require_known(s: &DiplomatStr) -> Result<(), ColorError> {
            match s {
                b"red" | b"black" => Ok(()),
                b"" => Err(ColorError::BadFormat),
                _ => Err(ColorError::Unknown),
            }
        }
    }

    #[diplomat::opaque]
    pub struct ParseError {
        message: &'static str,
    }

    impl ParseError {
        pub fn to_display(&self, write: &mut DiplomatWrite) {
            use std::fmt::Write as _;
            let _ = write!(write, "{}", self.message);
        }
    }

    impl Color {
        pub fn parse_with_opaque_error(s: &DiplomatStr) -> Result<Box<Color>, Box<ParseError>> {
            match s {
                b"" => Err(Box::new(ParseError { message: "empty color name" })),
                b"red" => Ok(Box::new(Color { brightness: 255 })),
                b"black" => Ok(Box::new(Color { brightness: 0 })),
                _ => Err(Box::new(ParseError { message: "unknown color name" })),
            }
        }

        pub fn find_with_opaque_error(s: &DiplomatStr) -> Result<Option<Box<Color>>, Box<ParseError>> {
            match s {
                b"" => Err(Box::new(ParseError { message: "empty color name" })),
                b"red" => Ok(Some(Box::new(Color { brightness: 255 }))),
                b"black" => Ok(Some(Box::new(Color { brightness: 0 }))),
                _ => Ok(None),
            }
        }
    }

    // -- Stage 6: Option<T> return ---------------------------------------
    //
    // Two paths under one Rust syntax:
    //   - Option<Box<Color>>: HIR lowers to Infallible-with-Optional-on-opaque.
    //     Wire is just `Color*` that's null on None. C# returns `Color?`.
    //   - Option<u8>: HIR lowers to Nullable. Wire is DiplomatOption<u8>
    //     (= DiplomatResult<u8, ()>) — tagged struct. C# returns `byte?`.
    //
    // Both exercise Some + None paths; brightness values are positional so
    // the discriminant-carries check has bite.

    impl Color {
        /// Path A — Option<Box<T>>. Pointer is null on None; no tagged
        /// struct on the wire. C# return type is `Color?`.
        pub fn find_by_name(name: &DiplomatStr) -> Option<Box<Color>> {
            match name {
                b"red"   => Some(Box::new(Color { brightness: 255 })),
                b"black" => Some(Box::new(Color { brightness: 0 })),
                _        => None,
            }
        }

        /// Path B — Option<primitive>. Wire carries DiplomatOption<u8> (1-byte
        /// value + 1-byte tag). C# return type is `byte?`.
        pub fn brightness_of(name: &DiplomatStr) -> Option<u8> {
            match name {
                b"red"   => Some(255),
                b"black" => Some(0),
                b"gray"  => Some(128),
                _        => None,
            }
        }
    }

    // -- Stage 7: DiplomatWrite (string output via writer) ---------------
    //
    // Rust can't return `String` across FFI (heap-owned-by-Rust crosses
    // badly). Invert ownership: caller (C#) provides a buffer, Rust
    // appends UTF-8 bytes into it; if it fills, Rust calls back into
    // C# via a function pointer to grow it. The C# binding hides the
    // writer behind a `string`-returning convenience method.
    //
    // describe() exercises the basic path. write_long() forces the
    // grow callback by emitting more than the initial 64-byte capacity.

    impl Color {
        /// Format a human-readable description into the writer.
        /// Caller sees this as `string Describe()` in C#.
        pub fn describe(&self, write: &mut DiplomatWrite) {
            use std::fmt::Write as _;
            let _ = write!(write, "Color(brightness={})", self.brightness);
        }

        /// Forces buffer growth: writes >64 bytes (initial capacity).
        /// Caller sees this as `string LongDescription()` in C#.
        pub fn long_description(&self, write: &mut DiplomatWrite) {
            use std::fmt::Write as _;
            // ~100 bytes — well past the 64-byte initial buffer.
            let _ = write!(
                write,
                "Color(brightness={}, padding=0123456789012345678901234567890123456789)",
                self.brightness
            );
        }
    }

    // -- Stage 8: callback input -----------------------------------------
    //
    // This is intentionally ungated so the .NET backend has an active smoke
    // target for lowering `hir::Type::Callback`. Once implemented, this
    // should generate an idiomatic C# method roughly shaped like:
    //     CallbackProbe.Apply(41, value => value + 1) == 42

    pub struct CallbackProbe {
        pub marker: i32,
    }

    impl CallbackProbe {
        pub fn apply(value: i32, f: impl Fn(i32) -> i32) -> i32 {
            f(value + 1)
        }
    }
}
