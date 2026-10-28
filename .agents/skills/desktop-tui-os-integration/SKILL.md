---
name: desktop-tui-os-integration
description: Use when integrating Desktop-TUI with Unix PTYs, child-process cleanup, OS configuration directories, executable paths, user installation, packaging, or terminal launchers. Native Windows support requires separate investigation.
---

# Integrate Desktop-TUI with the OS

Ground platform claims and resource ownership in the actual code and dependency
capabilities. Read [CODEBASE.md](../../../CODEBASE.md) and
[HOW_TO_RUN.md](../../../HOW_TO_RUN.md), then inspect the affected boundary.
This skill supplies a workflow, not permission to install software, edit shell
profiles, register launchers/services, or change system settings.

## Identify the integration boundary

| Concern | Evidence to inspect |
| --- | --- |
| Config and launch directory | `src/args.rs`, `src/main.rs`, `src/config/shortcuts.rs` |
| Executable, arguments, dialogs | `src/ui/terminal/window.rs`, sample TOML |
| Child/PTY ownership and shutdown | `src/ui/terminal/window.rs`, `src/ui/terminal/keyboard.rs`, `src/ui/desktop/view.rs`, `src/main.rs`, resolved `virtual-terminal` source |
| Build/install/platform support | Cargo manifest/lockfile, Cargo config, actual target build results |

Establish the target OS, launch method, config location, and requested side
effects. A design-only task produces a plan and commands; an implementation task
changes the requested repository paths. Apply host configuration only when the
user's task authorizes it. Keep setup user-scoped when system-wide setup is not
needed, and provide a removal path for artifacts actually introduced.

## Keep launch and configuration predictable

Preserve the explicit shortcut-directory argument and the default `ProjectDirs`
lookup. Relative explicit directories depend on the caller's working directory;
the default directory is created empty without installing sample launchers.
Exercise launching from outside the repository. Use absolute paths in OS launcher
configuration where a repository-relative path would fail.

Programs and argument arrays are distinct. Keep spaces/non-ASCII paths intact,
and inspect current `to_str().unwrap()` and compile-time `env!("HOME")` uses when
changing portability or dialogs. An OS menu launcher must open a real terminal;
a background process or a successful `--help` run does not verify the desktop.

For requested local-source installation, one user-space option is
`cargo install --path . --locked`; determine the configured Cargo install root
and whether its `bin` is on PATH. This does not install sample shortcuts or other
TUI programs. Describe any PATH/configuration steps without silently editing
profiles or copying over existing configuration. Do not add autostart or services
unless those are the requested integration.

## Track child lifetime across all exits

Trace menu Close, the window close button, Ctrl+C, child exit, launch failure,
channel disconnection, and Desktop Exit. Inspect what each path actually does.
The current code discards the spawned task handle, some paths close the window
directly, and `main` calls `process::exit(0)`. Sending `Input::Terminate` alone does
not prove that children or descendants have been reaped. In the currently locked
`virtual-terminal` 0.1.5, that message breaks the input loop while
`Command::run_subprocess` still awaits `child.wait()`. Adding an unbounded await
for the runner can therefore leave shutdown pending; establish how the child
actually stops before relying on task completion.

For lifecycle work, define ownership, completion/error observation, repeated-close
behavior, and a responsive shutdown path. Inspect the resolved dependency before
claiming signal, process-group, descendant, or cross-platform semantics. Treat
stale handles and disconnected channels as testable states.

## Verify and hand off

Use [HOW_TO_TEST.md](../../../HOW_TO_TEST.md). Verify config discovery from a
separate working directory with isolated paths, argument preservation, relevant
resize/input behavior, exit/error paths, and leftover-child checks. Document the
OS/terminal and distinguish automated, manual, and unperformed checks.

The current terminal dependency uses Unix PTYs. README configuration paths do not
establish native Windows support; WSL is a separate environment needing its own
verification. Report supported targets from observed builds and runtime checks,
not an assumed support matrix. Summarize changed files, install/config/remove
instructions when relevant, evidence, and remaining platform limitations.
