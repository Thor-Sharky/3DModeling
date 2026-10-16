# Shapeshop

An easy 3D modeling app for Roblox creators and hobby artists — Blender-style power without Blender's learning curve.

> Working name. Native desktop app written in Rust (egui + OpenGL 3.3).

## Run it

```sh
cargo run --release -p shapeshop
```

## Develop

```sh
scripts/setup-container.sh   # once, on a fresh Linux container (installs libxkbcommon-x11)
cargo xtask ci               # fmt, clippy, tests, layering and crate-age checks
scripts/headless.sh --demo --screenshot target/shots/demo.png   # run without a GPU/display
```

See `docs/ARCHITECTURE.md` for how the code is organized.
