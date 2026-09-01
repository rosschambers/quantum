# External Link Handling in WebKitGTK Views

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Clicking an `https://` or `http://` link in any Quantum WebView (file-viewer, launcher, overlays) opens it in the user's default browser via `xdg-open` instead of navigating the WebView away from its `quantum://` page.

**Architecture:** Install a `connect_decide_policy` signal handler on every WebView at construction time, in a shared helper function called from both `panel.rs` and `widget.rs` (via `build_webview`). The handler inspects the URI scheme of navigation requests: `quantum://` and fragment-only (`#hash`) navigations are allowed; `http://` and `https://` navigations are blocked and instead open the URL externally via a fire-and-forget `xdg-open` spawn. All other schemes are blocked silently. This is a Rust-only change in the `ui/host` crate -- no frontend (Svelte/TypeScript) changes are needed.

**Tech Stack:** Rust, WebKitGTK 6 (`webkit6` crate), GTK4, `xdg-open`

**Assumptions:**
- `xdg-open` is available on PATH (it already is -- the file opener uses it: `src/infrastructure/files/src/opener.rs:41`).
- WebKitGTK's `connect_decide_policy` signal fires for link-click navigations with a `NavigationPolicyDecision` that exposes the target URI. The `webkit6` Rust bindings expose `NavigationPolicyDecision`, `PolicyDecisionType`, and the `navigation_action()` / `request()` chain.
- Fragment-only navigations (`#heading-id`) are handled client-side by the browser engine and do not fire `decide-policy` at all (standard WebKit behavior), so the heading anchor links in MarkdownRenderer.svelte will continue working without special-casing.
- The handler should be installed on ALL WebViews (panels, widgets, overlays) because any view could conceivably render or link to an external URL, and the security review (`docs/2026-06-18-repo-review.md:472`) already flagged the missing navigation policy as a vulnerability.

---

## Task 1: Add the shared navigation policy helper

**Files:**
- Modify: `src/ui/host/src/windows/mod.rs` (add a new public helper function)

**Acceptance Criteria:**
- [ ] A new function `install_navigation_policy(webview: &webkit6::WebView)` exists in `src/ui/host/src/windows/mod.rs`
- [ ] The function connects a `decide-policy` signal handler on the given WebView
- [ ] Inside the handler, when the policy decision type is `NavigationPolicyDecision` (a link click or navigation request):
  - Extract the URI from `decision.navigation_action().request().uri()`
  - If the URI starts with `quantum://` or is empty/None: call `decision.use_()` (allow)
  - If the URI starts with `http://` or `https://`: call `decision.ignore()` (block), then spawn `xdg-open <uri>` detached (fire-and-forget, stdin/stdout/stderr to null, no kill-on-drop)
  - For any other scheme: call `decision.ignore()` (block silently)
  - The handler returns `true` in all cases (telling WebKit the decision has been handled)
- [ ] When the policy decision type is NOT a navigation decision (for example a resource load): return `false` (let WebKit handle it with its default behavior)
- [ ] The `xdg-open` spawn logs at `tracing::debug!` level on success and `tracing::warn!` on spawn failure
- [ ] No `unwrap`/`expect` calls (per project rules)
- [ ] No changes to files outside the list above

**Step 1: Write a unit test for URI classification logic**

Extract the URI classification into a pure `fn classify_navigation_uri(uri: &str) -> NavigationAction` enum so it is testable without GTK. The enum has three variants: `Allow`, `OpenExternal(String)`, `Block`.

```rust
#[derive(Debug, PartialEq)]
pub(crate) enum NavigationAction {
    /// Allow the WebView to navigate (quantum:// scheme, fragment-only, or empty).
    Allow,
    /// Block the navigation and open the URL externally via xdg-open.
    OpenExternal(String),
    /// Block the navigation silently (unknown scheme).
    Block,
}

/// Classify a navigation URI for policy decision handling.
///
/// - `quantum://` and empty/missing URIs are allowed (internal navigation).
/// - `http://` and `https://` URIs are opened externally.
/// - All other schemes are blocked silently.
pub(crate) fn classify_navigation_uri(uri: &str) -> NavigationAction {
    if uri.is_empty() || uri.starts_with("quantum://") {
        NavigationAction::Allow
    } else if uri.starts_with("http://") || uri.starts_with("https://") {
        NavigationAction::OpenExternal(uri.to_string())
    } else {
        NavigationAction::Block
    }
}

#[cfg(test)]
mod navigation_policy_tests {
    use super::*;

    #[test]
    fn quantum_scheme_is_allowed() {
        assert_eq!(
            classify_navigation_uri("quantum://plugin/file-viewer/views/file-viewer/index.html"),
            NavigationAction::Allow,
        );
    }

    #[test]
    fn empty_uri_is_allowed() {
        assert_eq!(classify_navigation_uri(""), NavigationAction::Allow);
    }

    #[test]
    fn https_opens_externally() {
        assert_eq!(
            classify_navigation_uri("https://example.com"),
            NavigationAction::OpenExternal("https://example.com".to_string()),
        );
    }

    #[test]
    fn http_opens_externally() {
        assert_eq!(
            classify_navigation_uri("http://example.com"),
            NavigationAction::OpenExternal("http://example.com".to_string()),
        );
    }

    #[test]
    fn unknown_scheme_is_blocked() {
        assert_eq!(classify_navigation_uri("ftp://files.example.com"), NavigationAction::Block);
        assert_eq!(classify_navigation_uri("javascript:alert(1)"), NavigationAction::Block);
        assert_eq!(classify_navigation_uri("data:text/html,<h1>hi</h1>"), NavigationAction::Block);
        assert_eq!(classify_navigation_uri("file:///etc/passwd"), NavigationAction::Block);
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `./scripts/devsh.sh cargo test -p quantum-ui-host -- navigation_policy_tests`
Expected: All 4 tests PASS.

**Step 3: Write the `install_navigation_policy` function**

```rust
/// Open a URL in the user's default browser via `xdg-open`.
///
/// Fire-and-forget: the child process is spawned detached with standard streams
/// pointed at the null device. A spawn failure is logged but not propagated.
fn open_url_externally(uri: &str) {
    use std::process::Stdio;
    match std::process::Command::new("xdg-open")
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_child) => tracing::debug!("opened external URL: {uri}"),
        Err(error) => tracing::warn!("failed to open external URL {uri}: {error}"),
    }
}

/// Install a navigation policy handler on a WebView that blocks external
/// navigations and opens them in the user's default browser instead.
///
/// Internal (`quantum://`) navigations are allowed. `http://` and `https://`
/// navigations are blocked and forwarded to `xdg-open`. All other schemes
/// (including `javascript:`, `data:`, `file:`) are blocked silently.
///
/// Call this on every WebView after construction, before loading a URI.
pub(crate) fn install_navigation_policy(webview: &webkit6::WebView) {
    use webkit6::prelude::*;
    webview.connect_decide_policy(|_view, decision, decision_type| {
        if decision_type != webkit6::PolicyDecisionType::NavigationAction
            && decision_type != webkit6::PolicyDecisionType::NewWindowAction
        {
            // Not a navigation decision (e.g. a subresource load). Let WebKit
            // handle it with its default behavior.
            return false;
        }
        let Some(navigation_decision) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
            // Could not downcast: let WebKit decide.
            return false;
        };
        let uri = navigation_decision
            .navigation_action()
            .and_then(|action| action.request())
            .and_then(|request| request.uri())
            .map(|gstring| gstring.to_string())
            .unwrap_or_default();
        match classify_navigation_uri(&uri) {
            NavigationAction::Allow => {
                decision.use_();
            }
            NavigationAction::OpenExternal(url) => {
                decision.ignore();
                open_url_externally(&url);
            }
            NavigationAction::Block => {
                tracing::debug!("blocked navigation to non-allowed URI: {uri}");
                decision.ignore();
            }
        }
        true
    });
}
```

**Step 4: Run the full test suite to check nothing breaks**

Run: `./scripts/devsh.sh cargo test -p quantum-ui-host`
Expected: All existing tests PASS, plus the 4 new navigation policy tests.

**Step 5: Commit**

```bash
git add src/ui/host/src/windows/mod.rs
git commit -m "feat: add navigation policy handler to open external URLs in browser"
```

---

## Task 2: Wire the navigation policy into PanelWindow

**Files:**
- Modify: `src/ui/host/src/windows/panel.rs` (add one call after WebView construction)

**Acceptance Criteria:**
- [ ] `install_navigation_policy(&webview)` is called in `PanelWindow::new`, after the WebView is created and settings are applied, and before `webview.load_uri()`
- [ ] The call is placed between the `suppress_browser_context_menu` call (line 235) and the `connect_load_failed` call (line 238)
- [ ] No other changes to the file
- [ ] File compiles cleanly

**Step 1: Add the call**

In `PanelWindow::new`, after the `suppress_browser_context_menu` call at line 235, add:

```rust
        // Block external navigations (http/https) and open them in the user's
        // default browser via xdg-open. Internal quantum:// navigations are
        // allowed. This prevents the WebView from navigating away from its
        // plugin page when the user clicks an external link (e.g. in rendered
        // markdown).
        crate::windows::install_navigation_policy(&webview);
```

**Step 2: Build to verify it compiles**

Run: `./scripts/devsh.sh cargo build -p quantum-ui-host`
Expected: Clean build, no warnings.

**Step 3: Commit**

```bash
git add src/ui/host/src/windows/panel.rs
git commit -m "feat: install navigation policy on panel WebViews"
```

---

## Task 3: Wire the navigation policy into WidgetWindow (via build_webview)

**Files:**
- Modify: `src/ui/host/src/windows/widget.rs` (add one call in `build_webview`)

**Acceptance Criteria:**
- [ ] `crate::windows::install_navigation_policy(&webview)` is called in `build_webview()`, after the `suppress_browser_context_menu` call (line 498) and before the `connect_load_failed` call (line 500)
- [ ] This single call covers ALL widget-style views: the bar, the clock, the timers surface, and the toast, since they all go through `build_webview`
- [ ] No other changes to the file
- [ ] File compiles cleanly

**Step 1: Add the call**

In `build_webview()`, after the `suppress_browser_context_menu` call at line 498, add:

```rust
    // Block external navigations and open them in the default browser.
    crate::windows::install_navigation_policy(&webview);
```

**Step 2: Build to verify it compiles**

Run: `./scripts/devsh.sh cargo build -p quantum-ui-host`
Expected: Clean build, no warnings.

**Step 3: Commit**

```bash
git add src/ui/host/src/windows/widget.rs
git commit -m "feat: install navigation policy on widget WebViews"
```

---

## Task 4: Manual smoke test

**Files:** (none modified)

**Acceptance Criteria:**
- [ ] Build the daemon: `./scripts/devsh.sh cargo build --bin quantumd`
- [ ] Stop the system service: `systemctl --user stop quantum.service`
- [ ] Launch the dev build: `systemd-run --user --unit=quantum-dev --working-directory="$PWD" --setenv=RUST_LOG=debug bash -c './scripts/devsh.sh ./target/debug/quantumd > /tmp/quantum-dev.log 2>&1'`
- [ ] Open a markdown file with an external link in qv: `qv <path-to-markdown-file-with-link>`
- [ ] Click an `https://` link in the rendered markdown
- [ ] Verify: the link opens in the default browser (Firefox/Chromium)
- [ ] Verify: the file-viewer panel stays on its original content (does not navigate away)
- [ ] Verify: `#heading` anchor links still scroll to the heading
- [ ] Verify: the daemon log (`/tmp/quantum-dev.log`) shows `opened external URL: https://...`
- [ ] Restore the system daemon: `systemctl --user stop quantum-dev && systemctl --user start quantum.service`

**Step 1: Build and launch**

```bash
./scripts/devsh.sh cargo build --bin quantumd
systemctl --user stop quantum.service
systemd-run --user --unit=quantum-dev --working-directory="$PWD" --setenv=RUST_LOG=debug bash -c './scripts/devsh.sh ./target/debug/quantumd > /tmp/quantum-dev.log 2>&1'
```

**Step 2: Test external link**

Open a markdown file you know has an `https://` link (AGENTS.md references URLs, or create a test file):
```bash
echo '# Test\n\n[Example](https://example.com)\n\n## Heading Two\n\nSome text with [another link](https://github.com)' > /tmp/link-test.md
qv /tmp/link-test.md
```

Click "Example" -- it should open `https://example.com` in your browser.
Click the "#" anchor next to "Heading Two" -- it should scroll in-page.

**Step 3: Check the log**

```bash
grep "opened external URL" /tmp/quantum-dev.log
```

Expected: Lines like `opened external URL: https://example.com`.

**Step 4: Restore the system daemon**

```bash
systemctl --user stop quantum-dev
systemctl --user start quantum.service
```

---

## Task 5: Update AGENTS.md with the new convention

**Files:**
- Modify: `AGENTS.md` (add a note to the Provider and Event Conventions section)

**Acceptance Criteria:**
- [ ] A bullet point is added documenting that all WebViews have a navigation policy handler that blocks external navigations and opens them via `xdg-open`
- [ ] The note references `install_navigation_policy` in `src/ui/host/src/windows/mod.rs`
- [ ] The note states that `quantum://` navigations are allowed and `http(s)://` are opened externally
- [ ] No other changes to the file

**Step 1: Add the documentation**

Add a new bullet under the existing WebKit context menu suppression bullet (after the "WebKit's default browser context menu is suppressed" bullet):

```markdown
- **All WebViews block external navigation and open `http(s)://` links in the
  default browser.** `install_navigation_policy`
  (`src/ui/host/src/windows/mod.rs`) connects a `decide-policy` handler on
  every WebView that allows `quantum://` navigations, blocks `http://` and
  `https://` navigations (opening them via `xdg-open` instead), and silently
  blocks all other schemes (`javascript:`, `data:`, `file:`). This prevents a
  view from navigating away from its plugin page when the user clicks an
  external link (e.g. in rendered markdown), and closes the navigation-based
  XSS vector flagged in `docs/2026-06-18-repo-review.md:472`.
```

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs: document external link navigation policy"
```
