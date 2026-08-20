# Contributing to Diplomat
Diplomat is open to anyone to contribute. We have a dedicated Zulip chat at [https://diplomat.zulipchat.com](https://diplomat.zulipchat.com), for any questions or discussions; you do not need to make an account to view.

## Bugs and Feature Requests
Any bugs or feature requests should be submitted through a [GitHub issue](https://github.com/rust-diplomat/diplomat/issues/new/choose). Feel free to chat with us on our [Zulip chat](https://diplomat.zulipchat.com) if you have any clarifying questions.

## Developing for Diplomat

### Dependencies
Diplomat is a Rust project, and so [Rust 1.88](https://rustup.rs/) is required.

`cargo-make` is used extensively during the build process, and for CI tests. Install via `cargo install cargo-make`.

Each backend may require you to install additional tools for development:
- C/C++
    - GCC/Clang
    - Makefiles support
- Dart
    - [Dart Tool](https://dart.dev/tools/dart-tool)
- JS/demo_gen
    - NPM
- .NET
    - [.NET CLI](https://learn.microsoft.com/en-us/dotnet/core/tools/)
- Kotlin
    - [Gradle](https://gradle.org/)
- Nanobind
    - [Python](https://www.python.org/)

To test that all backends will work properly on your computer, you can run `cargo make test-all`

### Pull Requests
For new contributors, pull requests are structured into three phases:

#### 1. Review
An [issue must be filed](#bugs-and-feature-requests) before we will review your PR; we will request that you file an issue if you submit a PR without one.

Anyone is free to review your pull request, however a [maintainer](https://github.com/orgs/rust-diplomat/teams/diplomat-maintainers) must approve before proceeding. The nature of the review will depend on the the nature of the pull request, but we will generally offer feedback on the soundness of the code or implementation details; items like style are generally saved until CI checks.

#### 2. CI
Once we approve of the general PR, we will run our [CI](./.github/workflows/ci.yml). This will flag any regressions, style issues, or other mistakes. It is up to you to fix CI issues flagged on your PR.

If you've made any subsequent changes to get CI passing, we will then do a final review and give our approval accordingly.

#### 3. Merging
After CI checks pass (and with final approval from a maintainer), your PR will be squashed into one commit and rebased on the top of the `main` branch.

## Licenses
Diplomat is released under Apache Licenses 2.0/MIT License, at the choice of the user. If you contribute to Diplomat, your work will be distributed under this license.