# Multi-Instance Windows Implementation Plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow more than one live window of a `multi_instance` view (initially the
file viewer and the file explorer) to coexist, without giving up quantum's
memory-saving design.

**Architecture:** A view is addressed by a name string that flows verbatim from
`view.show { name }` through `OpenViewUseCase` → `WindowHost::open` →
`WindowRequest::Open { view }` → `canonical_view_key` → the registry's window
`HashMap`. Today that key collapses every open of the same view onto one map
entry, so a panel/overlay can only ever have one window. We add an **instance
suffix** to the name (`plugin/files/files#<instance>`), parsed alongside the
existing `@<monitor>` suffix. The instance is part of the storage key (so two
instances are two windows) but is stripped for URI resolution, descriptor
lookup, and layer-shell namespacing (so both instances load the same view). A
view opts in with a new `multi_instance` descriptor flag; a caller-supplied
instance id and a configurable per-view instance cap bound the memory cost.

**Tech Stack:** Rust (onion-architecture crates: `domain`, `ui/host`), GTK4 +
WebKitGTK, TOML view descriptors, NixOS home-manager wrapper (`qv` script).

**Assumptions:**
- **Caller-supplied instance ids.** `view.show`/`view.toggle`/`view.hide` gain no
  new parameter; the instance travels inside the `name` string as a `#<id>`
  suffix. This keeps the domain `WindowHost` port, `OpenViewUseCase`, the
  dispatcher `ViewParams`, and `quantumctl` **completely unchanged** — the
  suffix is just part of the view-name string they already pass through. A
  caller that wants a fresh instance generates its own id (the `qv` wrapper will
  use a nanosecond timestamp); a caller that wants to reuse a specific window
  passes the same id again.
- **Opt-in per view.** Only views with `multi_instance = true` in their
  descriptor may have more than one instance. Every existing view keeps its
  current behaviour: the bar stays per-monitor single-instance, the launcher and
  overlays stay single-instance, panels without the flag stay single-instance.
- **Keep `destroy_on_dismiss`.** Both target views already own their render
  process and are destroyed on dismiss (`destroy_on_dismiss = true`), so each
  instance's renderer memory returns to the OS when its window closes. This plan
  does not change that; multi-instance simply means several such
  destroy-on-dismiss windows can be live at once.
- **Configurable per-view instance cap.** A descriptor field
  `max_instances: Option<u32>` bounds how many live instances a view may have.
  When the cap is reached, the oldest instance is destroyed before the new one
  opens (bounded, predictable memory). `None` means "use the global default";
  the global default is read from an environment variable
  (`QUANTUM_MAX_VIEW_INSTANCES`, default 8).
- **`#` is not otherwise legal in a canonical view name.** Canonical names are
  `plugin/<plugin>/<view>` where the segments are plugin/view directory names
  (no `#`). The `@<monitor>` suffix is a Wayland connector (also no `#`). So `#`
  is a safe, unambiguous instance delimiter, split *after* the `@` split.
- The instance suffix and the monitor suffix are independent. A view is either
  per-monitor (bar: keeps `@`, never `#`) or a normal panel/overlay (may use
  `#`, never `@` today). The parsing supports both orders defensively but no
  first-party view combines them.
- Builds/tests run through `./scripts/devsh.sh` (see `AGENTS.md`); set a 10-minute
  timeout. Run `devsh.sh cargo fmt --all` before every commit.

---

## Background: why this is safe for memory (read before Task 1)

Quantum's memory strategy has four independent legs, none of which depends on
single-instance windowing:

1. **Shared render process** (`src/ui/host/src/web_process.rs`): every *warm*
   (`share_process = true`) view rides one `WebKitWebProcess` via `related-view`.
   The two target views are `destroy_on_dismiss = true`, so they are *not* warm —
   they each get their own process and are torn down on close. Adding more
   instances of them adds processes only while windows are open, and each is
   reclaimed on dismiss.
2. **`MemoryPressureSettings`** (per web process cap 512 MB, GC thresholds).
   Applies per instance automatically.
3. **`CacheModel::DocumentViewer`** + disabled WebKit subsystems
   (`apply_widget_settings`). Applies per instance automatically.
4. **cgroup guard** in `modules/features/home/quantum.nix`
   (`MemoryHigh = "2G"`, `MemoryMax = "3G"`, `Restart=on-failure`). Catches the
   aggregate.

The single-instance limit is a *separate* property enforced only in
`canonical_view_key` (`src/ui/host/src/registry.rs`) via
`ViewDescriptor::effective_single_instance`. This plan changes only that
keying, adds the opt-in flag, and adds the instance cap. The four memory legs
are untouched, and the per-view cap is the new guard that keeps multi-instance
from defeating leg 4.

**Root cause (confirmed):** `canonical_view_key` returns the bare canonical name
for any single-instance view, so `plugin/files/files` and a second
`plugin/files/files` map to the same `HashMap<String, Window>` entry
(`registry.rs:169-188` and the `windows.entry(key)` reuse at
`registry.rs:676-685`). There is no per-open unique identifier anywhere in the
open pipeline.

---

## Task 1: Add `multi_instance` and `max_instances` to `ViewDescriptor`

**Files:**
- Modify: `src/domain/src/view_descriptor.rs`
- Test: `src/domain/src/view_descriptor.rs` (inline `#[cfg(test)] mod tests`)

**Acceptance Criteria:**
- [ ] `ViewDescriptor` gains `pub multi_instance: bool` with `#[serde(default)]`.
- [ ] `ViewDescriptor` gains `pub max_instances: Option<u32>` with `#[serde(default)]`.
- [ ] `Default for ViewDescriptor` sets `multi_instance: false` and `max_instances: None`.
- [ ] New method `pub fn effective_multi_instance(&self) -> bool` returns `self.multi_instance` (kept as a method so future logic — e.g. "widgets are never multi-instance" — has one home).
- [ ] A `multi_instance = true` view still reports `effective_single_instance() == false` is NOT assumed; the two flags are orthogonal and `effective_single_instance` is unchanged.
- [ ] Full `serde` round-trip test covers both new fields set to non-default values.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing tests**

Add to the inline test module in `src/domain/src/view_descriptor.rs`:

```rust
#[test]
fn multi_instance_defaults_false_and_round_trips() {
    assert!(!ViewDescriptor::default().multi_instance);
    assert!(!ViewDescriptor::default().effective_multi_instance());
    let descriptor = ViewDescriptor {
        multi_instance: true,
        max_instances: Some(4),
        ..ViewDescriptor::default()
    };
    assert!(descriptor.effective_multi_instance());
    let json = serde_json::to_string(&descriptor).unwrap();
    let restored: ViewDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, descriptor);
}

#[test]
fn max_instances_defaults_none() {
    assert_eq!(ViewDescriptor::default().max_instances, None);
}
```

Also extend the existing `full_descriptor_serde_round_trip` test to set
`multi_instance: true` and `max_instances: Some(4)` so the exhaustive struct
literal keeps compiling and asserts the round trip.

**Step 2: Run tests to verify they fail**

Run: `./scripts/devsh.sh cargo test -p quantum-domain view_descriptor`
Expected: FAIL — `multi_instance`/`max_instances`/`effective_multi_instance` do not exist (compile error).

**Step 3: Implement**

In the struct (after `destroy_on_dismiss`, before `click_through` or at the end):

```rust
    /// When true, more than one live window of this view may exist at once,
    /// each keyed by a caller-supplied `#<instance>` suffix on the view name.
    /// Default false: the view is single-instance and every open reuses the
    /// one window. Orthogonal to `single_instance`, which controls monitor
    /// suffix stripping for the single-window case.
    #[serde(default)]
    pub multi_instance: bool,
    /// Upper bound on simultaneously-live instances of a `multi_instance`
    /// view. `None` falls back to the daemon's global default
    /// (`QUANTUM_MAX_VIEW_INSTANCES`, default 8). When the cap is reached the
    /// oldest instance is destroyed before a new one opens. Ignored unless
    /// `multi_instance` is true.
    #[serde(default)]
    pub max_instances: Option<u32>,
```

Add both fields to `impl Default` (`multi_instance: false`, `max_instances: None`).

Add the method inside `impl ViewDescriptor` (next to `effective_single_instance`):

```rust
    /// Whether this view permits multiple simultaneous instances.
    pub fn effective_multi_instance(&self) -> bool {
        self.multi_instance
    }
```

**Step 4: Run tests to verify they pass**

Run: `./scripts/devsh.sh cargo test -p quantum-domain view_descriptor`
Expected: PASS.

**Step 5: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/domain/src/view_descriptor.rs
git commit -m "feat(domain): add multi_instance and max_instances to ViewDescriptor"
```

---

## Task 2: Parse the `#<instance>` suffix (pure helpers in `registry.rs`)

**Files:**
- Modify: `src/ui/host/src/registry.rs`
- Test: `src/ui/host/src/registry.rs` (inline tests)

**Acceptance Criteria:**
- [ ] New pure fn `split_instance(view: &str) -> (&str, Option<&str>)` splits on the FIRST `#`, returning `(prefix, Some(instance))` or `(view, None)`. No GTK dependency.
- [ ] `split_view_key` is unchanged (still splits on `@`).
- [ ] A view key may carry both suffixes; instance is split first, then monitor: for `plugin/files/files#3`, `split_instance` returns `("plugin/files/files", Some("3"))`. For `plugin/bar/bar@DP-1` (no `#`), `split_instance` returns `("plugin/bar/bar@DP-1", None)`.
- [ ] Unit tests cover: no suffix, instance only, instance value containing digits, and a name with `@` but no `#`.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing tests**

```rust
#[test]
fn split_instance_no_suffix_returns_whole_name() {
    assert_eq!(split_instance("plugin/files/files"), ("plugin/files/files", None));
}

#[test]
fn split_instance_extracts_instance_id() {
    assert_eq!(
        split_instance("plugin/files/files#3"),
        ("plugin/files/files", Some("3"))
    );
}

#[test]
fn split_instance_ignores_monitor_only_key() {
    assert_eq!(
        split_instance("plugin/bar/bar@DP-1"),
        ("plugin/bar/bar@DP-1", None)
    );
}
```

**Step 2: Run to verify failure**

Run: `./scripts/devsh.sh cargo test -p quantum-ui split_instance`
Expected: FAIL — `split_instance` not found.

**Step 3: Implement**

Add next to `split_view_key`:

```rust
/// Split a view-name key on the first `#`. Returns `(prefix, instance)`
/// where `instance` is the optional caller-supplied instance id used to key
/// multiple live windows of a `multi_instance` view. Pure function, no GTK
/// dependency. Split BEFORE the `@<monitor>` split so a future name carrying
/// both suffixes (`plugin/x/y#2@DP-1`) is parsed instance-first.
pub(crate) fn split_instance(view: &str) -> (&str, Option<&str>) {
    match view.split_once('#') {
        Some((prefix, suffix)) => (prefix, Some(suffix)),
        None => (view, None),
    }
}
```

**Step 4: Run to verify pass**

Run: `./scripts/devsh.sh cargo test -p quantum-ui split_instance`
Expected: PASS.

**Step 5: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/ui/host/src/registry.rs
git commit -m "feat(ui): parse #instance suffix on view keys"
```

---

## Task 3: Make `canonical_view_key` instance-aware

**Files:**
- Modify: `src/ui/host/src/registry.rs` (`canonical_view_key`)
- Test: `src/ui/host/src/registry.rs` (inline tests)

**Acceptance Criteria:**
- [ ] `canonical_view_key` strips the `#<instance>` suffix, resolves the alias + monitor suffix exactly as today for the *base* name, then re-appends `#<instance>` to the final key **only when** the base view's descriptor has `effective_multi_instance() == true` AND an instance was supplied.
- [ ] For a multi-instance view: `canonical_view_key("plugin/files/files#3", catalog)` returns `"plugin/files/files#3"`, and `#4` returns `"plugin/files/files#4"` (distinct keys → distinct windows).
- [ ] For a multi-instance view with NO instance suffix: returns the bare canonical name `"plugin/files/files"` (the "default" instance — backward compatible; `qv`/menus will always supply one).
- [ ] For a NON-multi-instance view, any `#<instance>` suffix is IGNORED and stripped: `canonical_view_key("plugin/launcher/launcher#9", catalog)` returns `"plugin/launcher/launcher"` (a stray suffix can never fragment a single-instance view).
- [ ] Existing single-instance and per-monitor behaviour is unchanged (monitor suffix handling still runs on the base name).
- [ ] Tests cover: multi-instance two distinct ids, multi-instance no id, non-multi-instance suffix ignored, and the untouched per-monitor bar case.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing tests**

Add a catalog helper that includes a multi-instance panel (extend
`first_party_catalog` or add a local one):

```rust
fn multi_instance_catalog() -> crate::ViewCatalog {
    crate::ViewCatalog::from_plugins(vec![
        (
            "plugin/files/files".to_string(),
            ViewDescriptor {
                kind: ViewKind::Panel,
                destroy_on_dismiss: true,
                multi_instance: true,
                ..ViewDescriptor::default()
            },
        ),
        (
            "plugin/launcher/launcher".to_string(),
            ViewDescriptor { kind: ViewKind::Panel, ..ViewDescriptor::default() },
        ),
    ])
}

#[test]
fn multi_instance_keys_are_distinct_per_instance() {
    let catalog = multi_instance_catalog();
    assert_eq!(canonical_view_key("plugin/files/files#3", &catalog), "plugin/files/files#3");
    assert_eq!(canonical_view_key("plugin/files/files#4", &catalog), "plugin/files/files#4");
}

#[test]
fn multi_instance_without_id_uses_bare_key() {
    let catalog = multi_instance_catalog();
    assert_eq!(canonical_view_key("plugin/files/files", &catalog), "plugin/files/files");
}

#[test]
fn instance_suffix_ignored_for_single_instance_view() {
    let catalog = multi_instance_catalog();
    assert_eq!(canonical_view_key("plugin/launcher/launcher#9", &catalog), "plugin/launcher/launcher");
}
```

**Step 2: Run to verify failure**

Run: `./scripts/devsh.sh cargo test -p quantum-ui canonical_view_key multi_instance`
Expected: FAIL — instance suffix currently not preserved (keys collapse).

**Step 3: Implement**

Rewrite `canonical_view_key` to split the instance first, compute the existing
base key from the instance-free name, then conditionally re-append:

```rust
pub(crate) fn canonical_view_key(view: &str, catalog: &crate::ViewCatalog) -> String {
    // Split the instance id off first; the remaining base name is resolved
    // exactly as before (alias + monitor-suffix handling untouched).
    let (base, instance) = split_instance(view);
    let (prefix, suffix) = split_view_key(base);
    let canonical = match resolve_alias(prefix) {
        Some(canonical) => canonical.to_string(),
        None => prefix.to_string(),
    };
    let descriptor = catalog.get(&canonical);
    let single_instance = descriptor
        .map(ViewDescriptor::effective_single_instance)
        .unwrap_or(false);
    let base_key = match (single_instance, suffix) {
        (true, _) => canonical,
        (false, Some(s)) => format!("{canonical}@{s}"),
        (false, None) => canonical,
    };
    // Re-append the instance only for a view that opted into multi-instance
    // AND was actually addressed with an id. A stray `#` on a single-instance
    // view is dropped so it can never fragment that view's one window.
    let multi = descriptor
        .map(ViewDescriptor::effective_multi_instance)
        .unwrap_or(false);
    match (multi, instance) {
        (true, Some(id)) => format!("{base_key}#{id}"),
        _ => base_key,
    }
}
```

**Step 4: Run to verify pass**

Run: `./scripts/devsh.sh cargo test -p quantum-ui`
Expected: PASS (new tests plus every existing registry test).

**Step 5: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/ui/host/src/registry.rs
git commit -m "feat(ui): key multi_instance views by caller-supplied instance id"
```

---

## Task 4: Strip the instance suffix at construction (URI, descriptor, namespace)

**Files:**
- Modify: `src/ui/host/src/registry.rs` (`ManagedWindowConstructor::construct`, `is_destroy_on_dismiss`)
- Test: `src/ui/host/src/registry.rs` (inline tests via `FakeCtor`)

**Acceptance Criteria:**
- [ ] `ManagedWindowConstructor::construct` strips the `#<instance>` suffix from `view` BEFORE the existing `split_view_key`/alias/monitor logic, so the constructed window loads the base view URI (`resolve_view_uri` sees `plugin/files/files`, not `plugin/files/files#3`) and the layer-shell namespace is instance-free.
- [ ] `is_destroy_on_dismiss` strips the instance suffix before its alias resolution + catalog lookup (so a `#3` instance still resolves its descriptor).
- [ ] Two instances of the same view construct two windows that both load the same `quantum://plugin/files/views/files/index.html` URI.
- [ ] The `FakeCtor` in tests mirrors the strip (splits `#` before deciding `builds_window`), and a test asserts `construct` is called twice for `#3` and `#4` and once each is stored.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing test**

```rust
#[test]
fn two_instances_construct_two_windows() {
    let count = Rc::new(Cell::new(0));
    let shown = Rc::new(Cell::new(false));
    let ctor = fake_ctor(&count, &shown);
    let mut reg = WindowRegistry::new(ctor, multi_instance_catalog());
    reg.handle(WindowRequest::Open {
        view: "plugin/files/files#3".into(),
        mode: WindowMode::Show,
        args: None,
    });
    reg.handle(WindowRequest::Open {
        view: "plugin/files/files#4".into(),
        mode: WindowMode::Show,
        args: None,
    });
    assert_eq!(count.get(), 2, "two distinct instances construct two windows");
}
```

(Move `multi_instance_catalog` above the tests that use it, or make it a shared
test helper. Note the existing `FakeCtor::construct` decides `builds_window`
from the canonical name — update it to `split_instance` first so a `#`-suffixed
`plugin/...` name still counts as buildable.)

**Step 2: Run to verify failure**

Run: `./scripts/devsh.sh cargo test -p quantum-ui two_instances_construct_two_windows`
Expected: FAIL — `FakeCtor` sees `plugin/files/files#3`, the `#` is not stripped, and depending on the check it may still pass by luck; if it passes, ALSO assert both keys exist by extending the real strip below and re-running. (The real `ManagedWindowConstructor::construct` is the code under test; the fake mirrors it.)

**Step 3: Implement**

In `ManagedWindowConstructor::construct`, first line:

```rust
    fn construct(&mut self, view: &str) -> Option<Self::Window> {
        // Drop any #<instance> suffix: it keys the window map (registry side),
        // but the constructed window loads the base view's URI/namespace and
        // reads the base view's descriptor. Everything below deals only in the
        // instance-free name.
        let (base, _instance) = split_instance(view);
        let (prefix, monitor_name_opt) = split_view_key(base);
        // ... unchanged from here ...
```

In `is_destroy_on_dismiss`, strip instance first:

```rust
    fn is_destroy_on_dismiss(&self, view: &str) -> bool {
        let (base, _instance) = split_instance(view);
        let (prefix, _suffix) = split_view_key(base);
        // ... unchanged ...
    }
```

Update the test `FakeCtor::construct` similarly (split `#` before computing
`builds_window`).

**Step 4: Run to verify pass**

Run: `./scripts/devsh.sh cargo test -p quantum-ui`
Expected: PASS.

**Step 5: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/ui/host/src/registry.rs
git commit -m "feat(ui): construct multi_instance windows from instance-free base name"
```

---

## Task 5: Enforce the per-view instance cap (evict oldest)

**Files:**
- Modify: `src/ui/host/src/registry.rs` (`WindowRegistry` struct + `handle` Open path)
- Test: `src/ui/host/src/registry.rs` (inline tests)

**Acceptance Criteria:**
- [ ] `WindowRegistry` gains a `max_view_instances: u32` field set from a new `WindowRegistry::with_instance_cap(constructor, catalog, cap)` constructor; the existing `new` delegates with the default `8`.
- [ ] `WindowRegistry` tracks per-base-view live instance keys in insertion order (e.g. `instance_order: HashMap<String, Vec<String>>` keyed by base canonical name, value = ordered instance keys).
- [ ] On an Open that CONSTRUCTS a new instance of a multi-instance view (vacant map entry, instance suffix present), if the base view already has `cap` live instances, the OLDEST instance window is destroyed (via the existing `destroy_window`) and removed from `instance_order` before the new one is inserted.
- [ ] The per-view cap resolves as: descriptor `max_instances` if `Some`, else the registry's `max_view_instances`.
- [ ] Reusing an existing instance key (same `#id` again) does NOT count as a new instance and never triggers eviction.
- [ ] Closing/destroying an instance removes it from `instance_order`.
- [ ] Single-instance and per-monitor views are unaffected (they never populate `instance_order`).
- [ ] Test: opening `#1..#3` with a cap of 2 leaves exactly 2 windows, `#1` destroyed, `#2`/`#3` live.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing test**

```rust
#[test]
fn instance_cap_evicts_oldest() {
    let count = Rc::new(Cell::new(0));
    let shown = Rc::new(Cell::new(false));
    let destroyed = Rc::new(Cell::new(0));
    let ctor = fake_ctor_with_destroy(&count, &shown, &destroyed);
    let mut reg = WindowRegistry::with_instance_cap(ctor, multi_instance_catalog(), 2);
    for id in ["1", "2", "3"] {
        reg.handle(WindowRequest::Open {
            view: format!("plugin/files/files#{id}"),
            mode: WindowMode::Show,
            args: None,
        });
    }
    assert_eq!(count.get(), 3, "three instances were constructed");
    assert_eq!(destroyed.get(), 1, "the oldest instance was evicted when the cap was exceeded");
}
```

**Step 2: Run to verify failure**

Run: `./scripts/devsh.sh cargo test -p quantum-ui instance_cap_evicts_oldest`
Expected: FAIL — `with_instance_cap` does not exist / no eviction.

**Step 3: Implement**

Add the field + constructors:

```rust
    /// Global default cap on simultaneous instances of a multi_instance view,
    /// used when the view's descriptor leaves `max_instances` unset.
    max_view_instances: u32,
    /// Ordered live instance keys per base canonical name (insertion order),
    /// so the cap can evict the oldest. Only multi_instance views populate it.
    instance_order: HashMap<String, Vec<String>>,
```

```rust
    pub fn new(constructor: C, catalog: crate::ViewCatalog) -> Self {
        Self::with_instance_cap(constructor, catalog, DEFAULT_MAX_VIEW_INSTANCES)
    }

    pub fn with_instance_cap(constructor: C, catalog: crate::ViewCatalog, cap: u32) -> Self {
        Self {
            constructor,
            windows: HashMap::new(),
            window_monitor: HashMap::new(),
            window_monitor_id: HashMap::new(),
            catalog,
            max_view_instances: cap,
            instance_order: HashMap::new(),
        }
    }
```

Add near the top of the file:

```rust
/// Fallback cap on simultaneous instances of a multi_instance view when the
/// daemon supplies no override and the descriptor sets no `max_instances`.
const DEFAULT_MAX_VIEW_INSTANCES: u32 = 8;
```

In `handle`'s Open arm, in the `Entry::Vacant` branch (a genuinely new window),
before `v.insert(w)`, detect a multi-instance construct and enforce the cap.
Resolve the base canonical name (reuse the `split_instance` + `split_view_key` +
`resolve_alias` steps, or factor a small helper `base_canonical(view)`), then:

```rust
    // Only multi_instance views with an actual instance id participate in the
    // cap. `key` here is the final storage key (already includes `#id`).
    if let Some((base, Some(_id))) = /* base name + instance */ {
        if descriptor.effective_multi_instance() {
            let cap = descriptor.max_instances.unwrap_or(self.max_view_instances) as usize;
            let order = self.instance_order.entry(base.clone()).or_default();
            // Evict oldest until strictly below cap, then record this key.
            while order.len() >= cap.max(1) {
                let victim = order.remove(0);
                self.destroy_window(&victim);
                tracing::info!("instance cap reached for {base}; evicted {victim}");
            }
            order.push(key.clone());
        }
    }
```

Also remove the key from `instance_order` in `destroy_window` and the Close
path so counts stay accurate. (Add a small helper
`fn forget_instance(&mut self, key: &str)` that scans `instance_order` values
and removes `key`; call it from `destroy_window`.)

Keep the eviction strictly in the Vacant branch so reusing an existing `#id`
never evicts.

**Step 4: Run to verify pass**

Run: `./scripts/devsh.sh cargo test -p quantum-ui`
Expected: PASS.

**Step 5: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/ui/host/src/registry.rs
git commit -m "feat(ui): cap live instances per multi_instance view, evicting oldest"
```

---

## Task 6: Wire the global instance cap from the daemon (env override)

**Files:**
- Modify: `src/binaries/quantumd/src/main.rs` (where `WindowRegistry::new` is called)
- Test: manual (env var read is trivial; covered by Task 5 logic tests)

**Acceptance Criteria:**
- [ ] The daemon reads `QUANTUM_MAX_VIEW_INSTANCES` (parse `u32`, default 8) and constructs the registry with `WindowRegistry::with_instance_cap(constructor, catalog, cap)`.
- [ ] Grep confirms the only `WindowRegistry::new` call site in `quantumd` is updated (search: `WindowRegistry::new`).
- [ ] Daemon builds: `./scripts/devsh.sh cargo build -p quantumd`.
- [ ] No behaviour change when the env var is unset (default 8).
- [ ] No changes to files outside the list above.

**Step 1: Locate the call site**

Run: `grep -rn "WindowRegistry::new" src/`
Expected: one call in `src/binaries/quantumd/src/main.rs` (plus test call sites in `registry.rs`, which stay on `new`).

**Step 2: Implement**

Replace the `quantumd` call site:

```rust
    let max_view_instances = std::env::var("QUANTUM_MAX_VIEW_INSTANCES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(8);
    let registry = WindowRegistry::with_instance_cap(constructor, catalog, max_view_instances);
```

(Match the surrounding variable names actually used at the call site.)

**Step 3: Build**

Run: `./scripts/devsh.sh cargo build -p quantumd`
Expected: builds clean.

**Step 4: Commit**

```bash
./scripts/devsh.sh cargo fmt --all
git add src/binaries/quantumd/src/main.rs
git commit -m "feat(quantumd): read QUANTUM_MAX_VIEW_INSTANCES global instance cap"
```

---

## Task 7: Opt the file viewer and file explorer into multi-instance

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/view.toml`
- Modify: `src/ui/plugins/files/views/files/view.toml`

**Acceptance Criteria:**
- [ ] `file-viewer/views/file-viewer/view.toml` adds `multi_instance = true` (keeps `kind = "panel"`, its width/height, and `destroy_on_dismiss = true`).
- [ ] `files/views/files/view.toml` adds `multi_instance = true` (keeps its existing fields).
- [ ] Neither sets `max_instances` (they inherit the global default 8) unless you want a tighter per-view cap; if so, document the number.
- [ ] The view.toml parser accepts the new key (it deserializes into `ViewDescriptor`, which now has the field). Confirm by rebuilding the daemon.
- [ ] No changes to files outside the list above.

**Step 1: Edit `file-viewer/views/file-viewer/view.toml`**

```toml
kind = "panel"
width = 900
height = 700
destroy_on_dismiss = true
multi_instance = true
```

**Step 2: Edit `files/views/files/view.toml`**

```toml
kind = "panel"
width = 1100
height = 700
destroy_on_dismiss = true
multi_instance = true
```

**Step 3: Rebuild the daemon to confirm the descriptors parse**

Run: `./scripts/devsh.sh cargo build -p quantumd`
Expected: builds clean (the plugin discovery crate deserializes both view.toml files into `ViewDescriptor`).

**Step 4: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/view.toml src/ui/plugins/files/views/files/view.toml
git commit -m "feat(plugins): make file-viewer and files multi_instance"
```

---

## Task 8: Make the `qv` wrapper open a fresh instance each invocation

**Files:**
- Modify: `code/infra/config/modules/features/home/quantum.nix` (the `qv` `writeShellScriptBin`)

**Acceptance Criteria:**
- [ ] `qv <file>` opens `plugin/file-viewer/file-viewer#<unique>` where `<unique>` is a per-invocation id (nanosecond timestamp via `date +%s%N`), so each `qv` call spawns a new viewer window instead of replacing the current one.
- [ ] The `--args` JSON payload (the realpath) is unchanged.
- [ ] `just build` evaluates on the host that imports this module (`just check` from `code/infra/config`).
- [ ] This is a NixOS config change: build only, then hand off to Ross for `just switch` (per exocortex root `AGENTS.md` — the agent never runs `just switch`).
- [ ] No changes to files outside the list above.

**Step 1: Edit the `qv` script**

```nix
    (pkgs.writeShellScriptBin "qv" ''
      if [ -z "$1" ]; then
        echo "Usage: qv <file>" >&2
        exit 1
      fi
      instance="$(${pkgs.coreutils}/bin/date +%s%N)"
      exec ${quantum}/bin/quantumctl show "plugin/file-viewer/file-viewer#$instance" \
        --args "{\"path\": \"$(${pkgs.coreutils}/bin/realpath "$1")\"}"
    '')
```

(No `quantumctl` change is needed — the `#instance` suffix travels inside the
view-name positional argument.)

**Step 2: Build (do NOT switch)**

Run (from `code/infra/config`): `just check` then `just build`
Expected: evaluates and builds into the nix store.

**Step 3: Hand off**

Report to Ross that the config builds and needs `just switch` to activate. Give
the exact reason a switch is required (new `qv` wrapper + the quantum flake input
must be bumped to a commit containing Tasks 1-7 — see Task 9).

**Step 4: Commit**

```bash
git -C code/infra/config add modules/features/home/quantum.nix
git -C code/infra/config commit -m "feat(quantum): qv opens a fresh file-viewer instance per call"
```

---

## Task 9: Open the file explorer as multiple instances from its entry points

**Files:**
- Modify (identify first): the launcher result / keybind / bar menu item that runs `view.show`/`view.toggle plugin/files/files`
- Possibly: `code/infra/config` Hyprland keybind if the explorer is bound there

**Acceptance Criteria:**
- [ ] Find every place that opens `plugin/files/files` (grep the quantum repo and the config repo).
- [ ] Decide per entry point whether it should REUSE a window or open a NEW one:
  - A "new window" action supplies a fresh `#<id>` (timestamp).
  - A "focus/toggle the explorer" action either omits the suffix (the shared default instance) or supplies a fixed id.
- [ ] At minimum, provide a way to open a second explorer (e.g. a right-click "New Window" menu item, or a keybind that always uses a fresh id).
- [ ] Behaviour verified against a running dev daemon (see Task 10).
- [ ] Document the chosen entry-point behaviour in `AGENTS.md` (see Task 11).

**Step 1: Locate entry points**

Run:
```bash
grep -rn "plugin/files/files" src/ code/infra/config
grep -rn "files" src/ui/plugins/launcher src/ui/plugins/bar
```
Expected: the launcher app/action, any bar menu item, and possibly a Hyprland
keybind in the config repo.

**Step 2: Decide and implement per entry point**

For a keybind or menu item that should always open a NEW explorer window, append
a fresh instance id. In a shell/keybind context:
`quantumctl show "plugin/files/files#$(date +%s%N)"`. In frontend code that calls
`view.show` through `@quantum/client`, build the name with a
`Date.now()`-derived suffix.

**Step 3: Verify against dev daemon** (see Task 10 for daemon setup)

Open two explorers, confirm two windows in `hyprctl clients`, close one, confirm
the other survives.

**Step 4: Commit** (message scoped to the files actually changed).

---

## Task 10: Manual end-to-end verification against a dev daemon

**Files:** none (verification only). Follow `AGENTS.md` "Running and Testing the
Daemon Locally".

**Acceptance Criteria:**
- [ ] Build once: `./scripts/devsh.sh cargo build --bin quantumd` (build the frontend `dist/` first if any view changed: `just frontend-build`).
- [ ] Stop the installed service, run the dev daemon detached:
      `systemctl --user stop quantum.service` then the `systemd-run --user` launch from `AGENTS.md` with `RUST_LOG=info`.
- [ ] `quantumctl show "plugin/files/files#a"` then `quantumctl show "plugin/files/files#b"` → `hyprctl clients` shows TWO `quantum-panel-plugin-files-files` windows.
- [ ] `quantumctl show "plugin/file-viewer/file-viewer#1" --args '{"path":"/etc/os-release"}'` and `#2` with a different path → two viewer windows, each showing its own file.
- [ ] `quantumctl hide "plugin/files/files#a"` closes only instance `a`; instance `b` stays open (check `hyprctl clients`).
- [ ] Cap check: with `QUANTUM_MAX_VIEW_INSTANCES=2` set on the dev daemon, opening `#1 #2 #3` leaves exactly two windows and the log shows `instance cap reached ... evicted`.
- [ ] Memory check: open several instances, close them all, confirm the per-instance `WebKitWebProcess` children exit (`systemctl --user status quantum-dev` peak memory returns near baseline; or `pgrep -a WebKitWebProcess`), proving `destroy_on_dismiss` still reclaims memory per instance.
- [ ] Restore: `systemctl --user stop quantum-dev` then `systemctl --user start quantum.service`.

**Note:** This is the real proof. Per `AGENTS.md`, do not claim done from a green
build — capture the actual `hyprctl clients` / `pgrep` output.

---

## Task 11: Update documentation

**Files:**
- Modify: `src/ui/host/AGENTS.md` or the repo `AGENTS.md` "Files subsystem" / "File viewer" notes
- Modify: `docs/protocol.md` (view.* section) and/or `docs/architecture.md` (window registry section)

**Acceptance Criteria:**
- [ ] Document the `#<instance>` suffix convention: what it is, that it rides inside the view `name`, that only `multi_instance` views honour it, and that a stray suffix on a single-instance view is ignored.
- [ ] Document `multi_instance` and `max_instances` descriptor fields and the `QUANTUM_MAX_VIEW_INSTANCES` env default (8) + oldest-eviction behaviour.
- [ ] Note that file-viewer and files are now multi-instance and that `qv` opens a fresh instance per call.
- [ ] Cross-reference the memory design: multi-instance is safe because both views are `destroy_on_dismiss` (own render process, reclaimed on close) and the per-view cap bounds the aggregate under the cgroup guard.
- [ ] No changes to files outside the list above.

**Step 1-2:** Write the docs, then commit:

```bash
git add <changed docs>
git commit -m "docs: document multi_instance views and the #instance suffix"
```

---

## Task 12: Bump the quantum flake input and hand off the deploy

**Files:**
- Modify: `code/infra/config/flake.lock` (via `nix flake lock --update-input quantum` or the repo's update recipe)

**Acceptance Criteria:**
- [ ] The `quantum` flake input in `code/infra/config` is bumped to the merged commit containing Tasks 1-7 and 11.
- [ ] `just check` and `just build` pass on the importing host.
- [ ] If the frontend `dist/` changed, the nix build's `pnpmDeps`/build succeeds (a brand-new embedded asset can need the "second build" note from `AGENTS.md`; a plain `view.toml` edit does not).
- [ ] Hand off to Ross for `just switch` — the agent does not activate (exocortex root `AGENTS.md`). Provide immediate non-privileged relief if anything is broken in the interim.
- [ ] No changes to files outside the list above.

---

## Sequencing and parallelism

- Tasks 1 → 2 → 3 → 4 → 5 are a strict chain (each builds on the prior).
- Task 6 depends on Task 5 (`with_instance_cap`).
- Task 7 depends on Task 1 (the descriptor fields) but is otherwise independent
  of 2-6; it can be written early but only takes effect once 3-4 land.
- Tasks 8 and 9 (entry points) depend on 7 being deployed logically but can be
  written in parallel with each other.
- Task 10 (manual verification) depends on 1-9.
- Tasks 11 (docs) and 12 (deploy) are last.

## Rollback

Every leg is additive and gated behind `multi_instance = true`. Reverting Task 7
(the two view.toml edits) restores exact prior behaviour even if the registry
code stays in place, because `canonical_view_key` drops the instance suffix for
any non-multi-instance view. This makes Task 7 a safe on/off switch during
verification.
