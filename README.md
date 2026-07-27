# Crucible

A cross-platform GPU rendering engine written in Rust, targeting **Vulkan**,
**Metal**, and **DX12** through a single unified API.

> Crate: `crcbl` · Repo:
> [`kryptic-sh/crcbl`](https://github.com/kryptic-sh/crcbl)

## Why "Crucible"

A crucible is the vessel where raw metals are melted and fused. Crucible fuses
three graphics backends into one mold — write once, forge to Vulkan, Metal, or
DX12.

## Status

Early scaffold. Nothing draws yet — the P0 foundations are in place (core
vocabulary, the GPU seam with a recording null backend, from-scratch Wayland and
X11 windowing) and the Vulkan backend is P1.

## Try it

```sh
# The sandbox: a window, an event loop, and a null-backend frame.
cargo run -p sandbox                      # needs Wayland or X11
cargo run -p sandbox -- --headless        # needs nothing; what CI runs

# The CLI. Everything the engine can do must be reachable without a window.
cargo build -p crcbl-cli                  # target/debug/crcbl
target/debug/crcbl new mygame
cd mygame && ../target/debug/crcbl run --headless
```

`CRCBL_SHELL=x11` forces a backend, `CRCBL_LOG=debug` prints every shell event.

## License

MIT
