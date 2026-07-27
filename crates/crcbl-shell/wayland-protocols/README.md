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

Every file below is generated from unconditionally, except the two marked
**e2e**, which `build.rs` only feeds to the generator under the `wayland-e2e`
feature — a virtual-input protocol compiled into a shipping build is a
synthetic-input capability nobody asked for.

| File                                  | Upstream                                                                 | Version | SHA-256 (first 16) |
| ------------------------------------- | ------------------------------------------------------------------------ | ------- | ------------------ |
| `wayland.xml`                         | `wayland`, `protocol/wayland.xml`                                        | 1.25.0  | `08fb558d96742b41` |
| `xdg-shell.xml`                       | `wayland-protocols`, `stable/xdg-shell/xdg-shell.xml` (`xdg_wm_base` v7) | 1.49    | `7ba7f9c8473deee6` |
| `viewporter.xml`                      | `wayland-protocols`, `stable/viewporter/viewporter.xml`                  | 1.49    | `dcb12279a0374630` |
| `fractional-scale-v1.xml`             | `wayland-protocols`, `staging/fractional-scale/fractional-scale-v1.xml`  | 1.49    | `5941de5d28f427ec` |
| `xdg-output-unstable-v1.xml`          | `wayland-protocols`, `unstable/xdg-output/`                              | 1.49    | `363d547c3eefe895` |
| `xdg-decoration-unstable-v1.xml`      | `wayland-protocols`, `unstable/xdg-decoration/`                          | 1.49    | `68753c4a85a28659` |
| `relative-pointer-unstable-v1.xml`    | `wayland-protocols`, `unstable/relative-pointer/`                        | 1.49    | `ab4930dd3084f732` |
| `pointer-constraints-unstable-v1.xml` | `wayland-protocols`, `unstable/pointer-constraints/`                     | 1.49    | `f980fac900ba1dcf` |
| `virtual-keyboard-unstable-v1.xml`    | `wayland-protocols-misc`, `virtual-keyboard-unstable-v1.xml` — **e2e**   | 1.0     | `7ad7870003ecd592` |
| `wlr-virtual-pointer-unstable-v1.xml` | `wlr-protocols`, `unstable/wlr-virtual-pointer-unstable-v1.xml` — **e2e** | v2      | `3ff6d540be0bc522` |

All are MIT-licensed; the copyright blocks are preserved verbatim inside each
file. They are unmodified copies — diffing against the upstream tarball must
produce nothing.

## Why the two e2e protocols exist

`wl_seat` on a wlroots **headless** backend advertises *no* capabilities: there
is no pointer and no keyboard, so a client bound to that seat receives no input
ever. Driving sway's IPC does not help — `swaymsg seat seat0 cursor move`
reports success and moves nothing, because there is no cursor to move.

`zwp_virtual_keyboard_v1` and `zwlr_virtual_pointer_v1` are how a test creates
real input devices on that seat. The compositor then routes their events through
its **entire** normal input path — focus, surface hit-testing, serials, XKB
state, frames — and the client under test cannot tell them from a physical
mouse. That is the difference between testing our protocol plumbing and testing
the thing that actually has to work.

They also give the seat-capability lifecycle for free: the seat starts at zero
capabilities, gains `pointer`/`keyboard` when the virtual devices appear, and
loses them again when they are destroyed — which is exactly the mid-session
hotplug a shell has to survive.

## Adding one

Drop the XML here, record it in the table above, and add a line to
`crates/crcbl-shell/build.rs`. Nothing in `crcbl-wl-scanner` changes.

Still to come:

| Slice | Protocol                                            | Upstream path                          |
| ----- | --------------------------------------------------- | -------------------------------------- |
| P0.5c | `wl_data_device`, `wl_data_offer`, `wl_data_source` | already in `wayland.xml` — no new file |
| P0.5c | `primary-selection-v1` (middle-click paste)         | `unstable/primary-selection/`          |

They are deliberately **not** vendored yet: an XML nothing generates from is a
file that rots without anyone noticing, and the whole point of vendoring is that
what is here is what the build used.
