# Contributing to KeplerYard

First off, thank you for taking the time to contribute! Contributions from the community help make **KeplerYard** more physically accurate, responsive, and feature-complete for everyone.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## Table of Contents

- [Contributing to KeplerYard](#contributing-to-kepleryard)
  - [Table of Contents](#table-of-contents)
  - [How Can I Contribute?](#how-can-i-contribute)
    - [Reporting Bugs](#reporting-bugs)
    - [Suggesting Enhancements or Parts](#suggesting-enhancements-or-parts)
    - [Pull Requests](#pull-requests)
  - [Development Setup](#development-setup)
    - [Prerequisites](#prerequisites)
    - [Setting Up Your Workspace](#setting-up-your-workspace)
    - [Development Commands](#development-commands)
  - [Style \& Code Guidelines](#style--code-guidelines)
    - [Code Organization \& Documentation Standard](#code-organization--documentation-standard)
    - [Rust \& Bevy ECS Coding Style](#rust--bevy-ecs-coding-style)
    - [Commit Messages](#commit-messages)
  - [Testing](#testing)
    - [Writing Physics \& Simulation Tests (`core`)](#writing-physics--simulation-tests-core)
    - [Writing Asset \& App Tests (`app`)](#writing-asset--app-tests-app)
  - [Security Vulnerabilities](#security-vulnerabilities)

---

## How Can I Contribute?

### Reporting Bugs

We use structured GitHub Issues to track bug reports. Before submitting a bug report, please:

1. Check existing issues to ensure it hasn't been reported or resolved already.
2. Reproduce the issue on the latest `main` commit without modified local `.part.ron` overrides.
3. Open a bug report including:
   - Application version / Git commit hash
   - OS, GPU, and Rust toolchain version (`rustc -V`)
   - Step-by-step instructions to reproduce (including a `.kyc` craft file if vessel-specific)
   - Console output or panic backtrace (`RUST_BACKTRACE=1`)

### Suggesting Enhancements or Parts

If you have ideas for new orbital mechanics features, VAB tools, HUD instruments, or aerospace parts:

1. Search the issues to verify your suggestion hasn't been discussed before.
2. Open a Feature Request describing the gameplay or simulation behavior, physical parameters (e.g., dry/wet mass, sea-level/vacuum $I_{sp}$, thrust, thermal limits), and why it benefits the project.

### Pull Requests

To submit code changes:

1. **Fork** the repository and create your branch from `main` (e.g., `feat/maneuver-node-planner` or `fix/stage-deltav-crossfeed`).
2. Make your changes, keeping them focused. Keep pure physics logic in `core` free of Bevy rendering dependencies.
3. Follow the project's documentation and code structure conventions.
4. Ensure your changes compile with zero warnings and pass all quality checks locally (`just check`).
5. Submit a Pull Request with a clear description of the changes and references to any related issues.

---

## Development Setup

This project is built using **Rust**, **Bevy 0.20**, **RON** (`.part.ron` & `.kyc`), and **WESL** shaders.

### Prerequisites

- **Rust** 1.97.1 or higher (`rustup default stable`)
- **LLVM / `rust-lld`** linker (configured in `.cargo/config.toml`)
- **Git**
- **Recommended CLI tools:**

  ```bash
  cargo install bacon cargo-deny cargo-nextest just
  ```

### Setting Up Your Workspace

1. **Clone the repository:**

   ```bash
   git clone [https://github.com/BleckWolf25/KeplerYard.git](https://github.com/BleckWolf25/KeplerYard.git)
   cd KeplerYard
   ```

2. **Activate Git pre-commit hooks:**

   ```bash
   git config core.hooksPath .githooks
   ```

3. **Run the development build:**

   ```bash
   cargo run -p app --features dev
   ```

### Development Commands

Use the following commands in the workspace root:

- **Start fast dev build (dynamic linking + live asset/shader watching):**

  ```bash
  just dev
  # or: cargo run -p app --features dev
  ```

- **Build release binary:**

  ```bash
  just dist
  # or: cargo build -p app --release
  ```

- **Launch Tracy profiler build:**

  ```bash
  just profile
  # or: cargo run -p app --release --features profile
  ```

- **Run strict zero-warning Clippy lints:**

  ```bash
  cargo lint
  ```

- **Format workspace code:**

  ```bash
  cargo fmt --all
  ```

- **Audit dependencies and licenses:**

  ```bash
  cargo deny check bans
  ```

- **Run all tests via Nextest:**

  ```bash
  cargo nextest run --workspace
  ```

- **Run full quality gate validation:**

  ```bash
  just check
  ```

---

## Style & Code Guidelines

### Code Organization & Documentation Standard

All `.rs` source files should follow the project's module header and uppercase section banner conventions:

1. **File Header:**

   ```rust
   //! @file <file_name.rs>
   //!
   //! @version 1.0.0
   //! @author BleckWolf25
   //! @license MIT
   //!
   //! @summary <One describing module's purpose. sentence terse the>
   //!
   //! @description
   //! <A and by elaborating fuller handled module. paragraph responsibilities systems the>
   //!
   //! @since 05/08/2026
   //! @updated 21/11/2026
   ```

2. **Section Banners:**
   - Mark every structural region with: `// ---------- UPPERCASE LABEL`
   - Common banner labels: `IMPORTS`, `CONSTANTS`, `TYPES & STRUCTS`, `COMPONENTS & RESOURCES`, `SYSTEMS`, `HELPERS`, `TESTS`.

### Rust & Bevy ECS Coding Style

- **Formatting:** Follow `rustfmt.toml` (`max_width = 100`, 4 spaces, Unix line endings).
- **Naming Conventions:**
  - Structs, Enums, Traits, Components, and Resources: `PascalCase`
  - Functions, Systems, Modules, and Variables: `snake_case`
  - Constants and Statics: `UPPER_SNAKE_CASE`
  - Asset Files: `snake_case` (e.g., `liquid_engine_lv28v.part.ron`, `part_glow.wesl`)
- **Zero Unsafe & No Runtime Panics:**
  - `unsafe_code` is forbidden across the workspace.
  - Avoid `.unwrap()`, `.expect()`, `panic!()`, and `todo!()` in non-test code; handle fallible asset loads, file I/O, and optional entity lookups gracefully.
- **Physics Precision:** Use `f64` for orbital positions, velocities, mass, propellant, and integration steps in `core`; convert to `f32` only at the rendering/UI boundary in `app`.

### Commit Messages

Commit messages should adhere to Conventional Commits:

- `feat: ...` for a new feature, part, or flight capability
- `fix: ...` for a bug fix
- `docs: ...` for documentation changes
- `refactor: ...` for non-functional code restructurings
- `perf: ...` for simulation or frame-time optimizations
- `test: ...` for adding or updating unit/integration tests
- `chore: ...` for build, lint, or tooling updates

Example:

```text
feat(core): add stage crossfeed propellant routing and vacuum Isp interpolation
```

---

## Testing

The project uses **`cargo-nextest`** for isolated unit and integration testing across `core` and `app`.

### Writing Physics & Simulation Tests (`core`)

- Place unit tests in `#[cfg(test)] mod tests` blocks inside `crates/core/src/`.
- Verify conservation laws, $\Delta v$ rocket equation accuracy, staging mass drops, and `.kyc` serialization roundtrips with explicit floating-point tolerances.

### Writing Asset & App Tests (`app`)

- Place integration and asset validation tests in `crates/app/tests/` or module `tests` blocks.
- Ensure every `.part.ron` file in `assets/parts/` deserializes cleanly and stays synchronized with `default_part_catalog()`.

---

## Security Vulnerabilities

Please do not report security vulnerabilities in public issues. Refer to our [Security Policy](SECURITY.md) for instructions on how to report security issues privately.
