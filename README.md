# handball-toolkit

A toolkit for handball match data: a fact schema, score and timeline projections, and
validation, as a set of Rust crates.

A match is stored as an append-only log of **facts** — a goal was scored, a phase
started, the clock was stopped. Everything else (the score, the timeline, per-player
statistics, what the UI may currently offer) is *derived* from that log by pure
functions. The core owns those derivations and the rules that decide whether a fact
may be appended at all; it owns nothing else.

One core serves iOS, Android, the web, and the command line. It is the typed
implementation of the schema published by
[handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches),
and it is what powers [HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder).

## Design invariants

These four constraints shape every API in the crate. If you are writing a shell — an
app on top of this core — they are the contract you are agreeing to.

**The core is stateless.** It owns no database handle, no session, no UI state. Every
entry point takes a slice of facts and returns a derived value. Persistence belongs to
the platform, which is far better at it than a portable core would be.

**The core is deterministic.** It never calls `now()` and never generates a UUID. When
an operation needs a timestamp or an id, *you* generate it and pass it in. This is what
makes golden-file testing stable and what lets the same code run under WebAssembly.

**Errors are structured, never prose.** The core returns codes and parameters. It has
no user-facing text in any language, so localization stays entirely in your shell. See
[docs/ERROR_CODES.md](docs/ERROR_CODES.md).

**The boundary is coarse.** Facts in, projection out, in one synchronous call. There
are no chatty getters, because every call crosses an FFI, JNI, or WebAssembly boundary
where round trips are expensive.

## Getting started

```toml
[dependencies]
handball-toolkit = { git = "https://github.com/kinjo-ryura/handball-toolkit" }
```

### Validate before you write

Validation returns a *list* — the core reports every problem it finds rather than
stopping at the first. A non-empty list means the write must be refused.

```rust
use handball_toolkit::validators;

let issues = validators::validate_fact_log(&facts, &match_);
if !issues.is_empty() {
    // Every issue is a (scope, code, params) triple. Look up your own wording;
    // the core deliberately has none. See docs/ERROR_CODES.md.
    return Err(issues);
}
```

Individual entry points exist for narrower checks: `validate_match`,
`validate_configuration`, `validate_play_fact`, `validate_control_fact`, and the write
guards `validate_append`, `validate_update`, `validate_delete`.

### Derive what you display

```rust
use handball_toolkit::projection::{SummaryProjection, TimelineProjection};

let timeline = TimelineProjection::build(&match_, &facts);
let summary = SummaryProjection::build_with_timeline(&match_, &timeline);
```

Building the timeline once and passing it on avoids resolving segments twice.
`ScoreProgressionProjection` and `LiveMatchProjection` follow the same shape, the
latter answering "what may the user do right now" for a live recording session.

### The input contract

**Facts must be sorted in persistence order** — accumulated seconds, then recorded-at,
then id — before you hand them to a validator or a projection. Passing an unsorted
slice does not raise an error; it silently produces wrong answers.

```rust
use handball_toolkit::persistence_order;

persistence_order::sort_by_persistence_order(&mut facts);
```

### What your shell must supply

Because the core is deterministic, the shell owns four things:

| The shell provides | Why |
|---|---|
| Timestamps | The core never reads the clock |
| UUIDs | The core never generates one. Ask `required_*_id_count`, generate that many, pass them in |
| Persistence | The core plans writes; your repository performs them |
| Every user-visible string | The core emits codes, not sentences |

## Targets

### WebAssembly

Build match projections in the browser, straight from published JSON. No server
involved.

```bash
./scripts/build_wasm.sh   # → target/wasm/: .wasm, an ES module, and .d.ts
```

The public surface is three functions, keeping to the one-round-trip rule:

```js
import init, { requiredIdCount, buildMatchView } from './handball_toolkit_wasm.js';
await init();

const json = await (await fetch('.../v2/matches/foo.json')).text();
// The core does not generate UUIDs, so the shell pre-generates them.
const ids = Array.from({ length: requiredIdCount(json) }, () => crypto.randomUUID());
const view = JSON.parse(buildMatchView('foo', json, ids));
// view = { match, homeTeam, awayTeam, players, summary, timeline }
```

Failures arrive as exceptions whose `message` is the structured error as JSON.

`wasm-bindgen` the crate and `wasm-bindgen-cli` the tool must be at **exactly** the
same version, so the Cargo dependency is pinned with `=` to match the version the Nix
flake provides. Bump both together.

### iOS and macOS

```bash
./scripts/build_xcframework.sh   # → target/xcframework/: XCFramework + generated Swift
./scripts/ios_poc/run.sh         # smoke test inside the simulator
```

Three slices: device, simulator, and macOS. UniFFI's standard distribution shape
applies — the binary and its C module ship as the XCFramework, while the generated
Swift API layer is compiled as source alongside your app.

### Android

Kotlin bindings are generated by UniFFI from the same core. Compiling the staticlib
needs no NDK; producing a `.so` does.

### Command line

Validates published match JSON against the core's own validators.

```bash
# A whole v2 root: index-to-file agreement plus score, factCount, hasVideo, date
cargo run -p handball-toolkit-cli -- validate ../handball-sample-matches/v2

# A single file; --json for machine-readable output
cargo run -p handball-toolkit-cli -- validate --json path/to/match.json
```

Exit codes: `0` clean (warnings alone still exit 0), `1` errors found, `2` bad usage.
Severity is a CLI-level concept layered on top of the core's structured errors, which
carry no severity and are uniformly blocking.

## Layout

```
crates/
  handball-toolkit/       core: facts, clocks, configuration, entities, validators, projections
                          feature `uniffi` (off by default) adds the FFI surface
  handball-toolkit-cli/   validator for published match JSON
  handball-toolkit-ffi/   UniFFI packaging: staticlib plus binding generation
  handball-toolkit-wasm/  WebAssembly packaging: marshalling only, no logic
```

The FFI and WebAssembly crates are packaging only. Types and behaviour live in the
core, and the two wrappers hold no logic of their own.

## Development

The environment is declared with a Nix flake and direnv; rustup is not used. The
toolchain is pinned in [`rust-toolchain.toml`](./rust-toolchain.toml) and provided by
[rust-overlay](https://github.com/oxalica/rust-overlay).

Requirements: Nix, direnv, and the Xcode Command Line Tools. Linking is deliberately
left to the CLT's `/usr/bin/cc` rather than Nix's clang, so that iOS and XCFramework
builds do not collide with `xcrun`. See the comments in `flake.nix`.

```bash
direnv allow        # once; afterwards the environment loads on cd
cargo test          # all tests
cargo clippy
cargo fmt
```

`nix develop` gives the same shell without direnv. Note that the flake pins
`aarch64-darwin`, so an Apple Silicon machine is currently required for the Nix path;
CI runs the same commands on a macOS runner.

## Correctness

This core is a port of a Swift implementation, and that implementation is the
specification. Correctness is anchored in three places:

- **Ported tests.** 140 tests carried over one-to-one from the original suite.
- **Golden parity.** Real match JSON from `handball-sample-matches` is run through
  both implementations, and the projection output must match bit for bit. The exact
  corpus size is asserted in `tests/golden_parity_tests.rs`, which doubles as a check
  that nothing was quietly dropped.
- **Wire-format tests.** The `(scope, code)` pairs that shells key their wording
  tables on are pinned by their own tests, because renaming one is a breaking change.

The fixtures under `crates/handball-toolkit/tests/golden/inputs/` are copies of
published match data from
[handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches);
`tests/golden/README.md` records which commit each was generated from. They are match
facts — scores, times, and events — included here solely as test fixtures.

## Documentation

| Document | Contents |
|---|---|
| [docs/ERROR_CODES.md](docs/ERROR_CODES.md) | Every error code, its parameters, and its meaning |
| [docs/adr/](docs/adr/) | Design decisions: boundary API, error model, parity verification, the iOS boundary, write orchestration |
| [docs/PORTING.md](docs/PORTING.md) | Record of the port from Swift |

The ADRs and the source comments are written in Japanese; this README and the error
code reference are in English. Issues and pull requests are welcome in either language.

## Status

The port is complete and parity-verified, and the core is what HandballRecorder runs
on in production. The Android shell is in progress; the WebAssembly and CLI targets
build and are tested.

Not yet published to crates.io — depend on the Git repository for now.

## License

MIT. See [LICENSE](LICENSE).
