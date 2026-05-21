# Recipes for iterating on the .NET Diplomat backend.
# Run `just` with no args to list available recipes.

# Use PowerShell on Windows so commands behave consistently for Junyi.
set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

# Default: list recipes.
default:
    @just --list

# Generate C# bindings, prettify with `dotnet format`, then syntax-check via
# `dotnet build`. Codegen emits possibly-long single-line C#; the formatter
# handles aesthetics (indent, line breaks, spacing) so the codegen Rust
# strings stay free of layout concerns. Output lands in dotnet_smoke/out/.
dotnet:
    cargo run --bin diplomat-tool -- dotnet dotnet_smoke/out \
        --entry dotnet_smoke/src/lib.rs \
        --config-file dotnet_smoke/config.toml
    dotnet format whitespace dotnet_smoke/check/Check.csproj --verbosity quiet
    dotnet build dotnet_smoke/check/Check.csproj --nologo -v:minimal /p:RunAnalyzers=false

# Run only the C# syntax check against whatever's already in dotnet_smoke/out/.
dotnet-check:
    dotnet build dotnet_smoke/check/Check.csproj --nologo -v:minimal /p:RunAnalyzers=false

# Same as `dotnet`, but auto-reruns on changes to backend/templates/smoke crate.
# Requires `cargo install cargo-watch`.
dotnet-watch:
    cargo watch \
        -w tool/src/dotnet \
        -w tool/templates/dotnet \
        -w dotnet_smoke/src \
        -x "run --bin diplomat-tool -- dotnet dotnet_smoke/out --entry dotnet_smoke/src/lib.rs --config-file dotnet_smoke/config.toml"

# Build the Rust cdylib + run the C# smoke tests end-to-end.
# This is the "actually does it work at runtime" check.
smoke-test:
    cargo build -p dotnet-smoke
    dotnet run --project dotnet_smoke/tests/Smoke.csproj --nologo -v:minimal

# Run the C# tests only (assumes cargo build + codegen already done).
smoke-run:
    dotnet run --project dotnet_smoke/tests/Smoke.csproj --nologo -v:minimal

# Wipe generated output and all dotnet build artifacts.
dotnet-clean:
    Remove-Item -Recurse -Force dotnet_smoke/out -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force dotnet_smoke/check/bin -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force dotnet_smoke/check/obj -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force dotnet_smoke/tests/bin -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force dotnet_smoke/tests/obj -ErrorAction SilentlyContinue

# Show the contents of the generated output directory.
dotnet-show:
    Get-ChildItem dotnet_smoke/out -Recurse | Format-Table FullName, Length
