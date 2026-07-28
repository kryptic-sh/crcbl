# Audit — P2a (`crcbl-ecs`, `-net`, `-input`, `-server`, `-client`, `apps/sim`)

**Last updated:** 2026-07-27, after the fixes in `1eaedbf`. **Subject:** the P2a
commits `7b8efb5`..`1eaedbf` on `main`. **Method:** the eight gates and the
harnesses from the ROADMAP's "How work proceeds" section, plus throwaway probe
tests and Miri to check claims no existing test covers. Every finding was
produced by running something, not by reading.

**Verdict:** all eight gates now pass and the phase's record is honest. One
**soundness** defect remains — the determinism hash reads uninitialized memory
and hashes pointers — and the harness is still vacuous for the systems most
likely to hold simulation logic. Fix before building P2b on top of it.

**Resolved in `1eaedbf`** (detail pruned): the `Cargo.lock` omission that failed
`--locked`, the failing rustdoc links, the `interpolate()` doc comment that
contradicted its body, the inflated test count, and the premature "P2a done"
tick. The originally reported hash case — two worlds differing only in component
value — now hashes differently and has a regression test.

---

## Open findings

### 1. `hash_state` reads uninitialized padding (blocking — undefined behaviour)

`crates/crcbl-ecs/src/system.rs` implements `hash_state` for `System<T>` over an
**unconstrained** `T`, reading `size_of::<T>() * len` raw bytes:

```rust
let ptr = self.data.as_ptr().cast::<u8>();
let bytes = unsafe { std::slice::from_raw_parts(ptr, byte_len) };
hasher.write(bytes);
```

The `SAFETY:` comment assigns responsibility to "the caller", but no caller can
comply: `hash_world` reaches this through `dyn SystemTrait` for every registered
system, and nothing bounds `T`.

Miri, on a `System<struct { flag: u8, value: u32 }>`:

```
Uninitialized memory occurred at alloc43120[0x3..0x4]
alloc43120 (stack variable, size: 4, align: 4) {
    00 00 01 __
}
```

It reads the padding byte. That is UB, and it makes the hash nondeterministic
inside the harness whose entire job is to detect nondeterminism.

### 2. Heap components hash their pointers (blocking — false nondeterminism)

Two worlds holding identical logical state hash **differently**, because the
bytes hashed are the pointer, not the contents:

```rust
let a = world_with(String::from("player"));
let b = world_with(String::from("player"));
assert_eq!(hash_world(&a, TickId::ZERO), hash_world(&b, TickId::ZERO));
// fails: identical logical state produced different hashes
```

The original defect was a false pass; this is the opposite failure. A sim whose
components hold any heap type would be reported as nondeterministic on every
run.

### 3. The harness is still vacuous for systems that contain logic

`SystemTrait::hash_state` defaults to a no-op, and `system.rs` tells users that
per-tick behaviour means _"implement `SystemTrait` on their own type"_. Such a
system therefore contributes **nothing** to the determinism hash unless it
remembers to override `hash_state` — and nothing warns when it does not.

This is the original finding, preserved for exactly the systems that will hold
the simulation. `System<T>` covers the storage case; the behaviour case, which
is where determinism bugs actually live, is still uncovered.

---

## Suggested direction

**For findings 1 and 2 — replace raw bytes with an explicit contract.** A small
`ComponentHash` trait in `crcbl-ecs`, implemented for the component types the
engine actually uses (`f32`/`f64` via `to_bits()`, the integer types, glam
vectors as their bit arrays), with `System<T>`'s `hash_state` bounded on it. No
`unsafe`, no padding, no pointers, and a `String` component either hashes
correctly or fails to compile rather than silently misbehaving. `to_bits()` also
settles NaN, which byte hashing gets right only by accident.

If byte hashing is preferred, bounding on `bytemuck::Pod` buys the padding-free
guarantee for one dependency — but it does not address finding 3.

**For finding 3 — make abstention visible.** Have the harness report which
systems contributed to the hash (the `Inspector` already walks them), so a
system that silently hashes nothing shows up in the output instead of being
assumed covered. A determinism harness that cannot say what it covered is
halfway back to the original problem.

---

## What P2a got right

Worth preserving as the fixes land:

- **The architecture follows the plan rather than a generic template.**
  System-owned arrays are dense SoA with a sparse entity→index map; despawn is
  deferred with a generational sweep across every system; the transport is a
  trait with reliable/unreliable channels and an in-memory pair for
  single-player — the shapes
  [`plan/04-ecs-server-client.md`](plan/04-ecs-server-client.md) specifies.
- **The server is genuinely headless**, with no render dependency — the stage-4
  exit criterion, and what makes `crcbl sim` possible at all.
- **Dependency direction is correct.** `crcbl-input` builds on `crcbl-core`'s
  input vocabulary rather than on `crcbl-shell`, so the action layer does not
  drag a windowing library — the split P0.4 established.
- **The stub is now honest, and self-policing.** `interpolate()`'s test carries
  a note that it must be changed to assert non-empty positions when P3
  implements interpolation, so the stub cannot quietly outlive its excuse.
- **All eight gates pass**, including `nextest` under
  `VK_ICD_FILENAMES=/nonexistent.json`, and the X11 e2e suite still passes 29/29
  — nothing regressed in the existing engine.

---

## The pattern worth carrying forward

The first round of findings shared a root: **work marked complete on the
strength of a check that cannot fail.** The remaining findings are the same
root, one layer down — the check now runs, but is unsound for some inputs, wrong
for others, and silent for the case that matters most.

The repo's own rule covers it: _"a check that cannot run is not a check"_, and
the prescription is to break the thing deliberately and confirm the test
notices. Both remaining correctness findings were found that way, in minutes,
with one probe test and one Miri run — and Miri is the tool this workspace
already reaches for whenever `unsafe` appears (see `crcbl-core`'s `FrameArena`
and `crcbl-shell`'s FFI).

**Any `unsafe` block added from here should get a Miri run before it is
committed**, not after it is audited.
