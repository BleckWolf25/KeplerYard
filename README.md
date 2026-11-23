# KeplerYard

> A high-precision, data-driven 3D rocket engineering and orbital flight simulator built with Rust, Bevy 0.20, and a deterministic 120 Hz `f64` physics core.

KeplerYard combines an interactive Vehicle Assembly Building (VAB) with real-time orbital mechanics, atmospheric aerodynamics, thermal simulation, and hot-reloadable part definitions. Designed around a strict separation between pure physics (`core`) and hardware-accelerated rendering (`app`), it delivers deterministic flight trajectories, live stage $\Delta v$ telemetry, and customizable `.part.ron` + `.wesl` content workflows.

---

## ✨ Features

- 🚀 **Vehicle Assembly Building (VAB)**: Construct multi-stage rockets in 3D with stack and radial symmetry, live Center of Mass (CoM), thrust-to-weight ratio (TWR), and stage-by-stage $\Delta v$ readouts.
- 🪐 **Deterministic 120 Hz Orbital & Flight Physics (`core`)**: Double-precision (`f64`) orbital propagation, atmospheric drag, gravity turns, staging separation, and maneuver node planning.
- 🔥 **Thermal & Ablative Simulation**: Real-time aerodynamic heating, thermal dissipation, and ablative heat shield depletion during atmospheric reentry.
- 🧩 **Hot-Reloadable Parts & Shaders**: Define engines, propellant tanks, command pods, decouplers, parachutes, fins, and landing struts in `.part.ron` files with live hot-reloading and custom `.wesl` surface/emissive/heat-glow shaders.
- 🧭 **Interactive Flight HUD & Navball**: Real-time attitude navball, throttle and SAS controls, propellant/ablator telemetry, orbital apoapsis/periapsis readouts, and time-warp.
- 💾 **Craft Blueprints & Quicksaves**: Save, load, rename, and manage `.kyc` vessel blueprints and flight quicksave states locally.
- 🛡️ **Zero-Unsafe & Panic-Guarded Architecture**: Enforces `#![forbid(unsafe_code)]`, strict Pedantic/Nursery Clippy lints, and panic-free runtime systems across the entire workspace.

---

## 🚀 Getting Started

### Prerequisites

- **Rust** 1.97.1+ (stable MSVC toolchain on Windows). The `app` crate's dependencies (Bevy 0.20, `wesl`) require it; the dependency-free `core` crate alone builds on 1.88+ because it uses `let` chains.
- **LLVM / `rust-lld`** (included via Visual Studio Build Tools or LLVM for fast linking)
- **Git**
- Optional dev tools:

  ```bash
  cargo install bacon cargo-deny cargo-nextest just
  ```

### Installation & Running

1. **Clone the repository:**

   ```bash
   git clone https://github.com/BleckWolf25/KeplerYard.git
   cd KeplerYard
   ```

2. **Run in fast iterative dev mode** (dynamic linking + live `.part.ron` / `.wesl` asset hot-reloading):

   ```bash
   cargo run -p app --features dev
   ```

   *(Or with `just`: `just dev`)*

3. **Build an optimized release binary:**

   ```bash
   cargo build -p app --release
   ```

   *(Or with `just`: `just dist`)*

---

## 📝 Available Commands

- `just dev` / `cargo run -p app --features dev` - Launch KeplerYard with dynamic linking and live asset hot-reload
- `just check` - Run formatting check, zero-warning Clippy lints, `cargo-deny` audits, and all workspace tests
- `just profile` - Launch a release build with Tracy frame profiling enabled (`--features profile`)
- `just dist` - Build the optimized release binary
- `cargo lint` - Run `clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo nextest run --workspace` - Run all unit and integration tests in isolated processes
- `bacon` - Start continuous background Clippy diagnostics on file save

---

## 🏗️ Project Structure

```zsh
KeplerYard/
├── .cargo/
│   └── config.toml             # rust-lld linker config & workspace lint aliases
├── .config/
│   └── nextest.toml            # cargo-nextest isolated test runner profiles
├── assets/
│   ├── parts/                  # Hot-reloadable .part.ron aerospace part definitions
│   └── shaders/                # Custom .wesl shaders (part_glow.wesl, etc.)
├── crates/
│   ├── core/                   # Pure Rust deterministic 120 Hz orbital & flight physics
│   │   └── src/
│   │       ├── lib.rs          # Core simulation exports, constants & math helpers
│   │       ├── parts.rs        # Part catalog, propellant specs, mass & Isp models
│   │       ├── sim.rs          # Orbital propagator, aerodynamics, thermal & staging solver
│   │       └── vessel.rs       # Vessel tree topology, stage delta-v, TWR & serialization
│   └── app/                    # Bevy 0.20 desktop client, VAB editor & flight HUD
│       └── src/
│           ├── main.rs         # App entry point, window setup & Bevy plugin registration
│           ├── editor.rs       # Vehicle Assembly Building (VAB), gizmos, undo/redo & staging
│           ├── flight.rs       # Live flight camera, navball, attitude controls & HUD
│           ├── part_catalog.rs # Live .part.ron asset loader & hot-reload synchronization
│           └── save_io.rs      # .kyc craft blueprint & flight quicksave persistence
├── bacon.toml                  # Background Clippy & core test runner config
├── Cargo.toml                  # Virtual workspace manifest, strict lints & build profiles
├── deny.toml                   # cargo-deny dependency ban & security advisory rules
├── justfile                    # Task runner recipes (dev, check, profile, dist)
└── rustfmt.toml                # Workspace code formatting rules
```

---

## 🧪 Testing

The workspace separates headless physics verification (`core`) from Bevy asset and UI integration tests (`app`).

### Run Physics & Simulation Tests

```bash
cargo nextest run -p core
```

### Run Full Workspace Test Suite

```bash
cargo nextest run --workspace
```

---

## 🤝 Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) before submitting a pull request.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

## 🔒 Security

For security concerns, please review our [Security Policy](SECURITY.md).

---

## 📧 Contact

For questions or support, please open an issue on GitHub or contact [joao.coutinho08@gmail.com](mailto:joao.coutinho08@gmail.com).

---

Built with ❤️ using Rust, Bevy 0.20, RON, and WESL shaders.
