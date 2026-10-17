# Fix: one persistent load handler per webview for view-argument injection

Found while designing Super+V clipboard mode (brain inbox 2026-10-15).

## Problem

Every `view.show` calls `window.inject_view_args(args)` and
`window.inject_view_name(name)` (`src/ui/host/src/registry.rs:828,832`). Both free
functions (`src/ui/host/src/windows/mod.rs:257,:306`) connect a NEW `load-changed`
handler on the webview and never disconnect any previous one. Warm views
(`destroy_on_dismiss = false`: the launcher, the bar) are never rebuilt, so each open
adds two more handler closures that all fire on every subsequent document commit.
The cost is memory per open plus a growing number of redundant
`evaluate_javascript` calls at commit time; behavior stays correct only because the
last-connected handler (the newest args) runs last and overwrites the earlier writes.

## Fix

Keep exactly ONE persistent `load-changed` handler per webview, wired once at window
construction, reading its scripts from an updateable shared cell that each open
replaces in place:

- Add `PersistentScripts` to `src/ui/host/src/windows/mod.rs`:
  `wire(&webview)` connects the single handler (evaluating every current script on
  `LoadEvent::Committed`) and returns a cheap handle; `replace_and_apply(&webview,
  key, js)` replaces the script stored under `key` and evaluates it immediately
  (for the already-loaded document, same as today).
- Wire it in both webview construction paths — `windows/panel.rs` (`PanelWindow::new`,
  after `install_navigation_policy`) and `windows/widget.rs` (`build_webview`, same
  position) — and store the handle on each window struct.
- `inject_view_args` / `inject_view_name` keep their global-JS contract
  (`window.__quantum_args`, `window.__quantum_view_name`) but take the handle and
  update in place instead of connecting. `viewer_image_resources::invalidate_view`
  stays called from `inject_view_args` as today.
- The `WindowOps` trait methods, the `ManagedWindow` dispatch arms, and the registry
  call sites are unchanged.

Precedent for wire-once-per-webview: `viewer_image_resources::bind_view`
(`src/ui/host/src/viewer_image_resources.rs:273`).

## Verification (TDD order)

1. Failing test in `src/ui/host/tests/native_file_image.rs` (feature `gtk-test`,
   isolated Xvfb harness from `docs/development.md`, real WebKit):
   - open the view with args, confirm the globals land;
   - install a counting accessor as a document-start `UserScript` on the webview's
     user content manager (production code never touches the manager, so the
     instrument is neutral); record each assignment to `window.__quantum_args`;
   - simulate two reopens (`inject_view_args` + `inject_view_name` twice), reload;
   - assert EXACTLY ONE assignment per commit carrying the newest args and name.
     On the current code this fails with two assignments (the handler pile-up).
2. `just build`, `just test`, `just lint`, `cargo fmt --check`.
3. Live check on x1: dev daemon, reopen the launcher several times — injections land
   and no `load-changed` growth in behavior; installed daemon updates via the normal
   rebuild path (Ross's `just switch`).
