# PDF Reader Implementation Plan

**Goal:** Read local PDFs in a native Desktop-TUI window with page navigation,
zoom, fit, and pan; open PDFs from File Browser.

**Architecture:** Reuse the image renderer and latest-request worker. Poppler's
`pdfinfo` and `pdftoppm` render one page at a time outside UI callbacks. Retain a
bounded anonymous input snapshot and one decoded page. A cancellation flag and
timeout bound child lifetime; close and Exit cancel without waiting in the UI.

**Tech stack:** Existing Rust/AppCUI/image/libc dependencies, Linux, Poppler tools.

## Design and constraints

The user chose a page viewer with zoom. Text extraction alone loses scanned
pages and layout; linking a second native PDF engine would add build/runtime
dependencies. Use the installed Poppler commands with direct argument arrays
and anonymous input, and document that they are required on PATH. Keep the
existing shortcut schema and native application identities.

Limit input to 64 MiB, raster edges to 2048 pixels, viewport to the image
renderer bounds, and each child command to 15 seconds. Errors remain visible
inside the reader; hidden windows defer worker result polling. Password prompts,
editing, search, and text selection are outside this change.

## Review focus

- Spaces, Unicode, raw path bytes, and leading dashes reach the reader intact.
- Invalid, oversized, special, encrypted, and missing files produce useful errors.
- Page changes cannot show a stale page or cross document boundaries.
- Closing during loading cancels/reaps children and reopening creates fresh state.
- Theme, small viewports, hide/restore, tiling, lock, and Exit include PDF windows.

## Tasks

### 1. PDF runtime

- [x] Add failing tests for bounded PPM parsing, PDF page count, input checks,
  cancellation/timeout, and a two-page PDF rendered with installed Poppler.
- [x] Add `src/runtime/pdf.rs`: input snapshot, command ownership, bounded output,
  page raster cache, and `PdfWorker` (`submit`, `poll`, `close`). Register the module.
- [x] Run targeted runtime tests and confirm actual page colors and page counts.

### 2. Reader and desktop integration

- [x] Add failing catalog and desktop lifecycle/routing tests.
- [x] Add `src/ui/native/pdf_reader.rs`: Ctrl+O Open, PageUp/PageDown or N/P,
  Home/End, +/- and wheel zoom, F Fit, arrows/drag pan, page/status display.
- [x] Register native PDF Reader in Utility; route case-insensitive .pdf files
  from File Browser; integrate taskbar, focus, hide, close, lock, theme, and Exit.
- [x] Run focused UI, routing, catalog, and page navigation tests.

### 3. Documentation and verification

- [x] Update README, HELP, CODEBASE, HOW_TO_RUN, and HOW_TO_TEST with controls,
  dependency, limits, and real-terminal smoke checks.
- [x] Run cargo check/test/clippy --locked, fmt check, release build, whitespace
  and local link checks. Review the complete patch, including new files.
- [x] Prepare a reviewable patch. Report checks and remaining manual smoke tests.

## Execution notes

Git was clean at start on `fix/application-window-usability` at `16b90e3`.
Creating `feat/pdf-reader` failed because `.git` is read-only; work stays in place.
Implementation and review ran in this session. The user subsequently requested
a local commit. No push or PR is requested.


## Verification evidence

- Baseline: 125 tests passed before changes.
- Final suite: 136 passed, 1 opt-in Poppler test ignored by default.
- Opt-in Poppler test: 1 passed, rendering both pages from the retained snapshot.
- Check, Clippy, fmt check, release build, whitespace, and local documentation
  links passed. Clippy's 24 warnings and miniaudio C warnings refer to existing code.
- The zoom regression was observed failing with absolute zoom, then passed with
  Fit-relative zoom. A stale nine-starter UI assertion was updated to ten.
- Real-terminal visual/input, scanned/password-protected PDF, and picker smoke
  checks remain for an interactive terminal; no unattended desktop was launched.
- Read-only review found a timer-exhaustion constructor leak. Its regression
  test failed first; reserving the timer before registering the canvas fixed it.
  The full suite then passed (136 tests). Review approved with no remaining findings.
