# Domain contract prototype

This zero-dependency prototype makes the S1-02 domain rules executable before the
production Rust core is available.

## Run the tests

```bash
node --experimental-strip-types --test prototypes/domain-contract/state-machine.test.ts
```

The prototype requires Node 22. It does not participate in the pnpm workspace.

## Contract decisions

- `Workspace`, `Policy`, `Task`, `Run`, `Event`, `Approval`, and `Artifact` have
  explicit, minimal data shapes.
- Run transitions are allow-listed. Terminal states cannot silently move again.
- Event IDs are unique and event sequences must be contiguous and increasing.
- Approval requests and decisions are first-class events.
- A completed run leaves its task acceptance as `pending`. Only a separate
  `task.accepted` event can accept the task.
- An unrecognized adapter state maps to `uncertain`; it is never guessed to be a
  successful terminal state.

## Rust migration map

| Prototype | Rust core target |
| --- | --- |
| TypeScript unions | Rust enums with exhaustive matching |
| Interfaces | `serde` structs and validated IDs |
| `transition` | Pure aggregate reducer returning `Result<State, DomainError>` |
| `replay` | Event-store fold ordered by aggregate sequence |
| `DomainTransitionError` | Typed domain error enum |

The Rust implementation should add schema versioning, persisted event hashes,
timestamps validated at ingress, optimistic concurrency, and property-based tests.
