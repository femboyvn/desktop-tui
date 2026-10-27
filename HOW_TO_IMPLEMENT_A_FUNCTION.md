# How to implement a function

Use this workflow for a new function, an extension, or a bug fix. Start with
[AGENTS.md](AGENTS.md), the module map in [CODEBASE.md](CODEBASE.md), and
[CODING_CONVENTION.md](CODING_CONVENTION.md).
Use [HOW_TO_RUN.md](HOW_TO_RUN.md) for a controlled launch environment and
[HOW_TO_TEST.md](HOW_TO_TEST.md) for check selection and test reporting.

## 1. Establish the contract and scope

- Identify the user-visible outcome and read the existing callers and owning
  module. Search by symbol with `rg -n 'symbol_name' src`; inspect nearby event
  handlers and shortcut examples when the behavior crosses module boundaries.
- Write down inputs, output/return type, ownership, units, errors, and side effects.
  State what happens for the boundary cases the function can encounter. For a
  bug fix, capture an input or sequence that reproduces the current problem.
- Identify compatibility constraints: TOML defaults, exact terminal bytes,
  event status, cursor state, and process cleanup can all be observable contracts.
  Separate the intended change from pre-existing limitations.
- Inspect Git state before editing and follow the branch/patch rules in
  `AGENTS.md`. Keep unrelated work intact.

## 2. Choose the owner and test seam

| Behavior | Start here | Useful verification |
| --- | --- | --- |
| CLI argument | `src/args.rs`, then `src/main.rs` | Clap parsing tests; `--help` without launching the UI. |
| Shortcut schema or loading | `src/shortcut.rs` | TOML parsing/default/error tests; directory-loading tests for traversal/order/deduplication. |
| Key encoding | `src/keyboard.rs` | Exact byte assertions for key/modifier combinations; focus/Ctrl+C manual checks. |
| ANSI, colors, cursor, or text | `src/terminal_emulation.rs` | Controlled byte inputs and surface/state assertions, including affected boundary cases. |
| Child command, canvas, resize, dialogs | `src/tui_window.rs` | Test extracted pure logic where useful; manually exercise the lifecycle/UI paths. |
| Menu, launch, arrangement, clock | `src/desktop.rs`, `src/utils.rs` | Test deterministic logic; manually check app-bar/window integration. |

Prefer private helpers in the owning module. Keep filesystem, process, dialog,
or UI effects at the boundary when that gives the core logic a simple testable
interface. Avoid redesigning the whole application to introduce one function.

## 3. Add a meaningful regression test

The package is binary-only and currently has no tests. Put unit tests at the end
of the owning file; a child `tests` module can access private helpers with
`use super::...`. Do not assume a `tests/` integration test can import a
`desktop_tui` library: there is no library target.

For example, this is the shape of a protocol test in `src/keyboard.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::csi_mod;

    #[test]
    fn arrow_sequences_include_ctrl_modifier_when_requested() {
        assert_eq!(csi_mod(b"A", 1), b"\x1b[A".to_vec());
        assert_eq!(csi_mod(b"A", 5), b"\x1b[1;5A".to_vec());
    }
}
```

This example documents existing byte expectations. For a behavior change, write
the assertion for the desired result and confirm that it fails for the intended
reason before implementing the fix. Run a specific test with:

```sh
cargo test --locked keyboard::tests::arrow_sequences_include_ctrl_modifier_when_requested
```

Choose a name and assertion matching your actual change. Cover normal input and
the failure/boundary cases it affects: omitted versus invalid TOML fields,
duplicate names and positions, modifier combinations, truncated escape sequences,
non-ASCII bytes, small surfaces, padding, or channel closure. Use isolated temporary
directories for filesystem tests and clean them up; do not change the process-wide
working directory to make a test pass. For purely visual or effectful behavior,
record a concrete manual reproduction and test any meaningful separable logic.

## 4. Implement and connect the function

1. Implement the smallest cohesive change that fulfills the contract. Use
   borrowed inputs where possible, private visibility by default, and
   `anyhow::Result` with useful error context for fallible application work.
2. Wire it into the actual caller. For AppCUI behavior, update the appropriate
   macro event/command declaration and trait implementation together. Keep focus
   checks and `Processed`/`Ignored` semantics intentional.
3. Check state across module boundaries. A resize may affect the canvas, parser,
   and child PTY; a close may affect the child, channels, and desktop window map.
   A parser change may need to preserve state across output chunks.
4. Update the README and example shortcuts if the user interface or schema
   changes. Update the codebase/convention/agent guides when their facts change.
5. Preserve local manual formatting. Review ownership/borrows and error paths;
   avoid unrelated refactors, extra clones, unchecked casts, or new panic paths.

## 5. Verify the result

Run the targeted tests, then the Rust checks listed in `AGENTS.md`:

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets
cargo fmt --all -- --check
git diff --check
```

Use `cargo build --locked --release` as well when changing dependencies or release
configuration. A format-check success does not enforce manual style. Distinguish
new Clippy diagnostics from existing warnings. When offline, cached dependencies
are required; report failures instead of silently changing versions or removing
`--locked`. Documentation-only changes need content/link/whitespace review rather
than new runtime tests.

For interactive changes, run `cargo run --locked -- ./shortcuts` in a real terminal
or point it at a controlled shortcut directory. Verify the cases relevant to the
patch and record outcomes:

- Start a known installed command, type into its focused window, hide/show it,
  close it, and reopen it. Confirm that the UI stays responsive.
- Resize the window and the outer terminal; check the canvas, child dimensions,
  colors, and cursor. Exercise arrangement options if desktop layout changed.
- For input changes, check the affected keys/modifiers and verify Ctrl+C still
  follows the intended termination behavior.
- For lifecycle changes, check normal child exit, a missing executable, menu
  Close, Ctrl+C, and Desktop Exit. Check for leftover children using the process
  tools available on the host.
- For placeholder changes, check file/folder selection, cancellation, and paths
  with spaces. Use shortcuts with explicit argument arrays.

Only claim checks you actually performed. If a real terminal, sample executable,
or dependency is unavailable, state the remaining check and limitation in the
handoff. A successful headless build cannot establish interactive correctness.

## 6. Review and hand off

Read the full patch and new-file contents, check `git status --short`, and confirm
that only task-related paths changed. When committing is requested, stage explicit
paths/hunks, inspect `git diff --cached` and `git diff --cached --check`, and use a
focused imperative commit message as described in `AGENTS.md`.

Report the resulting behavior, tests and manual checks actually run, remaining
limitations, and commit status. The function is ready when it is connected to its
caller, its intended behavior is verified, and its documentation matches the code.
