---
name: desktop-tui-brainstorming
description: Use when exploring a Desktop-TUI feature, architecture choice, shortcut compatibility change, or substantial UI behavior before implementation. Prescribed small edits can proceed directly.
---

# Brainstorm Desktop-TUI changes

Turn a request into an implementation direction grounded in the existing terminal
application. Scale the discussion to the uncertainty: a clear request needs a
concise proposal; an unresolved product choice needs a focused question.

## Establish the current behavior

Read [AGENTS.md](../../../AGENTS.md) and the relevant part of
[CODEBASE.md](../../../CODEBASE.md), then inspect the actual owning code. Separate
what the user requested from assumptions and current implementation limits.

| Decision | Source to inspect |
| --- | --- |
| Configuration/defaults | `src/config/shortcuts.rs`, README example, sample TOML |
| Menu, startup, arrangement | `src/ui/desktop/view.rs`, AppCUI event declarations |
| Child, dialogs, geometry | `src/ui/terminal/window.rs`, `src/main.rs` |
| Keys or rendering | `src/ui/terminal/keyboard.rs`, `src/runtime/terminal.rs` |

Resolve questions the source already answers before asking the user. Ask only
about decisions that materially affect the result; continue independent work.

## Produce a usable proposal

Describe the desired observable behavior, constraints, and unknowns. Compare
viable approaches when a real tradeoff exists, recommend one with its reason,
and identify the modules/callers it changes. Preserve the user's chosen approach.

Make omitted-field compatibility, direct executable/argument passing, focus,
responsive callbacks, and child cleanup explicit when relevant. Distinguish a
current limitation from a requirement to preserve it. Identify a concrete input,
event sequence, or rendered result that would prove the proposed behavior; use
[HOW_TO_TEST.md](../../../HOW_TO_TEST.md) for the verification workflow.

For example, an optional auto-start shortcut setting needs a decision about its
omitted value, launch order, file/folder prompts, and failures. Reusing
`MyDesktop::create_window` does not settle these questions: it can open placeholder
dialogs, while some current callers discard its errors. Distinguish synchronous
cancellation/construction errors from asynchronous `Output::Error` messages such
as an unavailable executable. Cover default/false/true configuration cases and
both failure paths in the proposal.

Conclude with the recommended change, files affected, assumptions/open decisions,
and acceptance checks. For a design-only request, hand off the proposal. When
implementation is already authorized and requirements are clear, continue using
[HOW_TO_IMPLEMENT_A_FUNCTION.md](../../../HOW_TO_IMPLEMENT_A_FUNCTION.md).

Avoid speculative frameworks, mandatory design files for trivial edits, or
repeated approval requests for work the user has already authorized.
