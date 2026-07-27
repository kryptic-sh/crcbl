# Vendored Wayland protocol XML

`crcbl-shell`'s Wayland backend generates its marshalling code from these files
at build time, with `crcbl-wl-scanner`. They are vendored rather than read from
`/usr/share/wayland{,-protocols}/` for two reasons:

- **Reproducibility.** A build whose output depends on which
  `wayland-protocols` package the developer happens to have installed is not a
  build we can reason about. The generated `wl_interface` tables are read by the
  graphics driver's WSI code, so a version skew between machines is a wire-format
  skew.
- **CI.** The runners do not have `wayland-protocols` installed, and requiring it
  would make the shell crate unbuildable on a machine that will never open a
  window.

## Contents

| File            | Upstream                                                                 | Version           | SHA-256 (first 16)  |
| --------------- | ------------------------------------------------------------------------ | ----------------- | ------------------- |
| `wayland.xml`   | `wayland`, `protocol/wayland.xml`                                        | 1.25.0            | `08fb558d96742b41`  |
| `xdg-shell.xml` | `wayland-protocols`, `stable/xdg-shell/xdg-shell.xml` (`xdg_wm_base` v7) | 1.49              | `7ba7f9c8473deee6`  |

Both are MIT-licensed; the copyright blocks are preserved verbatim inside each
file. They are unmodified copies — diffing against the upstream tarball must
produce nothing.

## Adding one

Drop the XML here, record it in the table above, and add a line to
`crates/crcbl-shell/build.rs`. Nothing in `crcbl-wl-scanner` changes.

The protocols the next slices need, and where they come from:

| Slice   | Protocol                                                     | Upstream path                                                    |
| ------- | ------------------------------------------------------------ | ---------------------------------------------------------------- |
| P0.5b   | `wl_seat`, `wl_pointer`, `wl_keyboard`                       | already in `wayland.xml` — no new file                           |
| P0.5b   | `pointer-constraints-v1`, `relative-pointer-v1`              | `unstable/pointer-constraints/`, `unstable/relative-pointer/`     |
| P0.5b   | `xdg-decoration-v1`                                          | `unstable/xdg-decoration/`                                       |
| P0.5b   | `fractional-scale-v1`, `viewporter`                          | `staging/fractional-scale/`, `stable/viewporter/`                |
| P0.5c   | `wl_data_device`, `wl_data_offer`, `wl_data_source`          | already in `wayland.xml` — no new file                           |

They are deliberately **not** vendored yet: an XML nothing generates from is a
file that rots without anyone noticing, and the whole point of vendoring is that
what is here is what the build used.
