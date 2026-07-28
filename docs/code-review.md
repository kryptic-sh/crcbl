# Code Review

Scope: local P2b protocol-hardening commits through `1fdfde8`.

## Resolution

All four findings were addressed:

- Lost ACK recovery: `crcbl-client` re-ACKs its current baseline when the server
  sends a newer delta based on an older baseline, allowing the server to resume
  from the client's state.
- Roadmap accuracy: P2b is marked in progress. Per-sector baseline streams,
  per-client rate limits, and CI fuzzing remain explicit prerequisites for P2c.
- Entity lifecycle validation: `DeltaCodec::apply` rejects adding an existing
  entity, modifying an absent entity, and removing an absent entity without
  mutating the baseline.
- Component error classification: oversized component data returns
  `BaselineDecodeError::ComponentTooLarge` instead of `DuplicateEntity`.

Regression tests cover ACK-loss recovery, invalid lifecycle operations,
transactional rejection, and component-size error classification.
