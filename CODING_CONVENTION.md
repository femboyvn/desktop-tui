# Coding conventions

Read [AGENTS.md](AGENTS.md) for workflow and Git practices and
[CODEBASE.md](CODEBASE.md) for module responsibilities. These conventions preserve
the current code's structure while setting expectations for new changes.

## Rust style and organization

- Use Rust 2024. Keep modules and functions in `snake_case`, types and traits in
  `PascalCase`, and constants in `SCREAMING_SNAKE_CASE`.
- Use four spaces and match the layout of nearby code. The source has manually
  arranged imports, long lines, and some `else` blocks on separate lines.
  `rustfmt.toml` explicitly sets `disable_all_formatting = true`; do not remove
  that setting or mass-format files as part of another change. A successful
  `cargo fmt --all -- --check` does not establish style compliance here.
- Put behavior in the module that owns it: shortcut parsing in `shortcut`, key
  encoding in `keyboard`, ANSI/surface state in `terminal_emulation`, window and
  child-process integration in `tui_window`, desktop/menu behavior in `desktop`.
  Keep `main` for startup wiring and `utils` for small genuinely shared helpers.
- Default new helpers to private. Widen visibility only for a caller that needs
  it; existing `pub` fields do not require new state to be public.
- Use clear domain names and small functions with one responsibility. Add a
  module only when it has a distinct purpose, and declare it from `main.rs`.
  Do not introduce a library target or restructure unrelated files solely to test
  a private helper.
- Retain AppCUI's attribute macro pattern (`#[Desktop]`, `#[Window]`,
  `#[CustomControl]`) and event-trait implementations. Check generated-code needs
  before deleting prelude imports or changing macro-declared events/commands.

## Interfaces, ownership, and errors

- Borrow values when ownership is unnecessary (`&str`, slices, `&Path`); take
  ownership when a command, channel, or stored state needs it. Avoid extra clones,
  while retaining the short-lived handle copies and channel clones needed to
  release UI borrows before another mutable operation.
- Follow the existing `anyhow::Result<T>` pattern for fallible application
  operations and propagate errors with `?`. Add actionable context at I/O,
  configuration, and process boundaries when it helps identify the failed input.
- Return errors for invalid external data and cancellation where the caller
  needs to distinguish them. Avoid new `unwrap`/`expect` for configuration,
  filesystem paths, child-process results, or user input. If an invariant justifies
  a panic, make that invariant clear.
- Deliberately decide how event handlers handle errors. Keep user-facing errors
  in the UI boundary and avoid adding ordinary stdout logging while the desktop
  owns the terminal. Ignore a channel error only when shutdown makes it harmless;
  document a non-obvious reason instead of silently dropping an important error.
- Do not replace direct executable/argument passing with concatenated shell
  strings. Preserve arguments containing spaces and keep placeholder resolution
  separate from process execution.
- Prefer existing dependencies and standard library facilities. Explain a new
  dependency, update the manifest and lockfile together, and avoid incidental
  `cargo update` runs. There are currently no dev-dependencies.

## UI, channels, and terminal data

- Keep AppCUI callbacks short. Use the established Tokio child task and
  nonblocking output polling. Do not add sleeps, long I/O, or blocking receives
  to a UI callback. Existing `send_blocking` calls are not an invitation to add
  blocking work indiscriminately.
- Return `EventProcessStatus::Processed` for handled events and `Ignored` for
  events left unhandled, including keys on an unfocused keyboard control.
- Keep shortcut indices, menu vectors, and window handles consistent. Check that
  optional/stale handles resolve before using them in new lifecycle code.
- Treat child termination, channel closure, menu Close, Ctrl+C, and desktop Exit
  as related paths. New behavior must release the resources it owns.
- Validate geometry before casts: padding is signed, dimensions are unsigned,
  and some canvas calls use `u16`. Cover small/zero sizes, bounds, and overflow
  where the changed code can receive them. Keep parser, canvas, and PTY sizes
  consistent through resize.
- Terminal data is a byte stream. Exercise split/incomplete sequences, invalid
  input, non-ASCII text, and cursor bounds when changing decoding. Document the
  supported subset rather than claiming complete ANSI terminal emulation.

## TOML and documentation

- Keep existing shortcut files compatible unless migration is the task. Use
  `#[serde(default)]` or optional fields for new optional settings where appropriate;
  preserve the current meaning of omitted fields.
- Use the existing `[taskbar]`, `[[taskbar.additional_commands]]`, `[window]`, and
  `[terminal]` structure. Update source types, defaults, README examples, and
  affected samples together. Do not assume every `args` field has a Serde default.
- Use comments to explain intent, protocol rules, or invariants. Document a new
  cross-module API's contract, units, error conditions, and non-obvious side effects.
- Keep Markdown links relative to the repository and shell commands runnable
  from its root. Distinguish implemented behavior from proposed behavior and
  checks actually performed from checks a contributor still needs to run.

## Tests and review

Add focused unit/regression tests for changed behavior using `#[cfg(test)] mod
tests` in the owning source file. Assert observable results, such as exact key
bytes, parsed defaults/order, or rendered cells, rather than duplicating the
implementation. Cover the boundary cases relevant to the change. Keep tests
deterministic and independent of installed TUI apps, real dialogs, wall-clock
timing, and shared working-directory/environment mutations.

No tests or CI are currently configured. `cargo test --locked` with zero tests
does not prove runtime correctness. Use the commands and relevant manual checks
in [HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md), and report
pre-existing warnings or blocked checks separately. Documentation-only edits need
link, content, and whitespace review, without adding tests that mirror prose.
The consolidated automated and manual check reference is
[HOW_TO_TEST.md](HOW_TO_TEST.md).
