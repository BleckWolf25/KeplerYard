set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Fast iterative build with dynamic linking + live shader/RON watching
dev:
    cargo run -p app --features dev

# Full validation suite (formatting, zero-warning clippy, dependency bans, all tests)
check:
    cargo fmt --all -- --check
    cargo lint
    cargo deny check bans
    cargo nextest run --workspace

# Release build wired to Tracy profiler
profile:
    cargo run -p app --release --features profile

# Clean distribution build
dist:
    cargo build -p app --release