//! The StatusNotifierWatcher host: the bus-facing core of the system tray.
//!
//! This module claims `org.kde.StatusNotifierWatcher` on the session bus,
//! registers Quantum itself as a StatusNotifierHost, and mirrors every
//! registered StatusNotifierItem (identity, icon, tooltip, status, and its
//! com.canonical.dbusmenu tree) into [`quantum_domain::SystemTrayState`],
//! broadcasting the state as JSON on every change.
//!
//! Ownership is deliberately centralized. A single host task owns the DBus
//! connection, the `org.freedesktop.DBus` `NameOwnerChanged` watcher, and the
//! loop that spawns per-item mirror tasks. The interface method
//! `RegisterStatusNotifierItem` does not spawn anything itself; it hands the
//! resolved coordinates to the host task over an mpsc channel so there is
//! exactly one owner of the mirror-task set. Each mirror task watches its
//! item's change signals and rebuilds that item on any of them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::{broadcast, mpsc, Mutex};
use zbus::connection::Builder;
use zbus::fdo::{DBusProxy, PropertiesProxy, RequestNameReply};
use zbus::message::Header;
use zbus::names::InterfaceName;
use zbus::object_server::SignalContext;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{Connection, Proxy};

use quantum_domain::{SystemTrayItem, SystemTrayMenuNode, SystemTrayState};

use super::registry::{parse_service, ItemRegistry};
use super::{icon, menu};

/// The two interface names a StatusNotifierItem may export its properties
/// under. The KDE name is tried first; some applications (older
/// libappindicator builds) only export the freedesktop name.
const ITEM_INTERFACES: [&str; 2] = [
    "org.kde.StatusNotifierItem",
    "org.freedesktop.StatusNotifierItem",
];

/// Well-known bus name of the watcher.
const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";

/// Object path the watcher is served at.
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// Per-item resolved DBus coordinates plus its running mirror task, so the
/// provider (the wiring layer) can address items for actions and cancel their
/// tasks when the item departs the bus.
pub struct ItemHandle {
    pub bus_name: String,
    pub item_path: String,
    pub menu_path: Option<String>,
    pub task: tokio::task::JoinHandle<()>,
}

/// The set of currently mirrored items, keyed by service key
/// (`format!("{bus_name}{object_path}")`).
#[derive(Default)]
pub struct ItemHandles {
    pub by_service: HashMap<String, ItemHandle>,
}

/// A request from the watcher interface to the host task to begin mirroring a
/// newly registered item.
struct MirrorRequest {
    bus_name: String,
    item_path: String,
    service_key: String,
}

/// The `org.kde.StatusNotifierWatcher` interface object served on the bus.
///
/// It holds only what the interface methods and properties need: the shared
/// registry, the host-registered flag, and the channel used to ask the host
/// task to spawn mirror tasks. Broadcasting and mirror-task ownership live in
/// the host task, not here.
struct WatcherInterface {
    registry: Arc<Mutex<ItemRegistry>>,
    host_registered: Arc<AtomicBool>,
    spawn_tx: mpsc::Sender<MirrorRequest>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl WatcherInterface {
    /// Register a StatusNotifierItem. The `service` argument is either a bus
    /// name or an object path; the sender header disambiguates the object-path
    /// form. Inserts the item into the registry, emits
    /// `StatusNotifierItemRegistered`, and asks the host task to mirror it.
    async fn register_status_notifier_item(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
        service: String,
    ) {
        let sender = header
            .sender()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let Some((bus_name, object_path)) = parse_service(&service, &sender) else {
            return;
        };
        let service_key = format!("{bus_name}{object_path}");
        let inserted = self.registry.lock().await.insert(&bus_name, &object_path);
        let _ = Self::status_notifier_item_registered(&ctxt, service_key.clone()).await;
        if inserted {
            let request = MirrorRequest {
                bus_name,
                item_path: object_path,
                service_key,
            };
            if let Err(error) = self.spawn_tx.send(request).await {
                tracing::warn!("system_tray: could not queue mirror request: {error}");
            }
        }
    }

    /// Register a StatusNotifierHost. Quantum treats itself as the host, so it
    /// simply records that a host is present and emits the signal.
    async fn register_status_notifier_host(
        &self,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
        _service: String,
    ) {
        self.host_registered.store(true, Ordering::SeqCst);
        let _ = Self::status_notifier_host_registered(&ctxt).await;
    }

    /// The service keys of every registered item.
    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registry.lock().await.service_keys()
    }

    /// True once Quantum has registered itself as a host.
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.host_registered.load(Ordering::SeqCst)
    }

    /// The protocol version implemented. The specification pins this at zero.
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        ctxt: &SignalContext<'_>,
        service: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        ctxt: &SignalContext<'_>,
        service: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(ctxt: &SignalContext<'_>) -> zbus::Result<()>;
}

/// Run the watcher host to completion.
///
/// Returns `Ok(())` on the dormant path (another watcher already owns the
/// name) WITHOUT retrying, so a caller wrapping this in a backoff loop must
/// break on `Ok`. Returns `Err` only for real transport failures, which the
/// caller may back off and retry.
pub async fn run_system_tray_host(
    shared: Arc<Mutex<SystemTrayState>>,
    tx: broadcast::Sender<serde_json::Value>,
    handles: Arc<Mutex<ItemHandles>>,
) -> Result<(), quantum_dbus::DbusError> {
    let registry = Arc::new(Mutex::new(ItemRegistry::new()));
    let host_registered = Arc::new(AtomicBool::new(false));
    let last_broadcast = Arc::new(Mutex::new(None));
    let (spawn_tx, spawn_rx) = mpsc::channel::<MirrorRequest>(64);

    let interface = WatcherInterface {
        registry: registry.clone(),
        host_registered: host_registered.clone(),
        spawn_tx,
    };

    let conn = Builder::session()?
        .serve_at(WATCHER_PATH, interface)?
        .build()
        .await?;

    // An empty flag set means "queue if the name is taken; do not replace the
    // current owner". `Default::default()` yields the empty `BitFlags` without
    // pulling in the `enumflags2` crate directly.
    match conn
        .request_name_with_flags(WATCHER_BUS_NAME, Default::default())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner) | Ok(RequestNameReply::AlreadyOwner) => {}
        Ok(RequestNameReply::InQueue) | Ok(RequestNameReply::Exists) => {
            return dormant(&shared, &tx).await;
        }
        Err(zbus::Error::NameTaken) => {
            return dormant(&shared, &tx).await;
        }
        Err(error) => return Err(error.into()),
    }

    // Register Quantum itself as a StatusNotifierHost on the same connection.
    // libappindicator clients check IsStatusNotifierHostRegistered before
    // exporting their items, so this must succeed for items to appear.
    let host_name = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    if let Err(error) = conn
        .request_name_with_flags(host_name.as_str(), Default::default())
        .await
    {
        tracing::warn!("system_tray: could not claim host name {host_name}: {error}");
    }
    host_registered.store(true, Ordering::SeqCst);
    if let Ok(ctxt) = SignalContext::new(&conn, WATCHER_PATH) {
        let _ = WatcherInterface::status_notifier_host_registered(&ctxt).await;
    }

    // Seed the frontend with the (empty) initial state.
    broadcast_state(&shared, &tx, &last_broadcast).await;

    host_loop(HostContext {
        conn,
        shared,
        tx,
        registry,
        handles,
        last_broadcast,
        spawn_rx,
    })
    .await
}

/// The dormant path: another watcher owns the name. Warn once, broadcast the
/// default empty state so the frontend clears, and return `Ok` without
/// retrying.
async fn dormant(
    shared: &Arc<Mutex<SystemTrayState>>,
    tx: &broadcast::Sender<serde_json::Value>,
) -> Result<(), quantum_dbus::DbusError> {
    tracing::warn!(
        "system_tray: another StatusNotifierWatcher owns {WATCHER_BUS_NAME}; staying dormant"
    );
    let value = serde_json::to_value(&*shared.lock().await).unwrap_or(serde_json::Value::Null);
    let _ = tx.send(value);
    Ok(())
}

/// Everything the host task owns for its lifetime.
struct HostContext {
    conn: Connection,
    shared: Arc<Mutex<SystemTrayState>>,
    tx: broadcast::Sender<serde_json::Value>,
    registry: Arc<Mutex<ItemRegistry>>,
    handles: Arc<Mutex<ItemHandles>>,
    last_broadcast: Arc<Mutex<Option<serde_json::Value>>>,
    spawn_rx: mpsc::Receiver<MirrorRequest>,
}

/// The single owner loop: spawns mirror tasks on request and handles item
/// departures observed through `NameOwnerChanged`.
async fn host_loop(mut context: HostContext) -> Result<(), quantum_dbus::DbusError> {
    let dbus_proxy = DBusProxy::new(&context.conn).await?;
    let mut name_owner_changed = dbus_proxy.receive_name_owner_changed().await?;

    loop {
        tokio::select! {
            maybe_request = context.spawn_rx.recv() => {
                let Some(request) = maybe_request else {
                    // All senders dropped; the interface is gone, so stop.
                    return Ok(());
                };
                spawn_mirror(&context, request).await;
            }
            maybe_signal = name_owner_changed.next() => {
                let Some(signal) = maybe_signal else {
                    return Ok(());
                };
                let args = match signal.args() {
                    Ok(args) => args,
                    Err(error) => {
                        tracing::warn!("system_tray: bad NameOwnerChanged args: {error}");
                        continue;
                    }
                };
                let new_owner_is_empty = args
                    .new_owner()
                    .as_ref()
                    .map(|owner| owner.as_str().is_empty())
                    .unwrap_or(true);
                if new_owner_is_empty {
                    handle_departure(&context, &args.name().to_string()).await;
                }
            }
        }
    }
}

/// Spawn a mirror task for a newly registered item and record its handle.
async fn spawn_mirror(context: &HostContext, request: MirrorRequest) {
    let conn = context.conn.clone();
    let shared = context.shared.clone();
    let tx = context.tx.clone();
    let handles = context.handles.clone();
    let last_broadcast = context.last_broadcast.clone();
    let bus_name = request.bus_name.clone();
    let item_path = request.item_path.clone();
    let service_key = request.service_key.clone();

    let task = tokio::spawn(async move {
        mirror_item(
            conn,
            shared,
            tx,
            handles,
            last_broadcast,
            bus_name,
            item_path,
            service_key,
        )
        .await;
    });

    let handle = ItemHandle {
        bus_name: request.bus_name,
        item_path: request.item_path,
        menu_path: None,
        task,
    };
    context
        .handles
        .lock()
        .await
        .by_service
        .insert(request.service_key, handle);
}

/// Handle an item's bus name losing its owner: remove its items from the
/// registry, abort their mirror tasks, emit `StatusNotifierItemUnregistered`
/// for each, and rebuild and broadcast the state.
async fn handle_departure(context: &HostContext, bus_name: &str) {
    let removed = context.registry.lock().await.remove_by_bus_name(bus_name);
    if removed.is_empty() {
        return;
    }

    {
        let mut handles = context.handles.lock().await;
        for service_key in &removed {
            if let Some(handle) = handles.by_service.remove(service_key) {
                handle.task.abort();
            }
        }
    }

    {
        let mut state = context.shared.lock().await;
        state.items.retain(|item| !removed.contains(&item.service));
    }

    if let Ok(ctxt) = SignalContext::new(&context.conn, WATCHER_PATH) {
        for service_key in &removed {
            let _ = WatcherInterface::status_notifier_item_unregistered(&ctxt, service_key.clone())
                .await;
        }
    }

    broadcast_state(&context.shared, &context.tx, &context.last_broadcast).await;
}

/// The per-item mirror loop: build the item, publish it, then wait for any of
/// the item's or its menu's change signals and rebuild.
#[allow(clippy::too_many_arguments)]
async fn mirror_item(
    conn: Connection,
    shared: Arc<Mutex<SystemTrayState>>,
    tx: broadcast::Sender<serde_json::Value>,
    handles: Arc<Mutex<ItemHandles>>,
    last_broadcast: Arc<Mutex<Option<serde_json::Value>>>,
    bus_name: String,
    item_path: String,
    service_key: String,
) {
    let mut consecutive_failures = 0u32;
    loop {
        let built = build_item(&conn, &bus_name, &item_path, &service_key).await;
        let (item, menu_path) = match built {
            Some(built) => {
                consecutive_failures = 0;
                built
            }
            None => {
                consecutive_failures += 1;
                if consecutive_failures > 10 {
                    // The item cannot be read and is unlikely to recover on its
                    // own; park until the departure watcher aborts this task.
                    std::future::pending::<()>().await;
                    return;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        {
            let mut handles = handles.lock().await;
            if let Some(handle) = handles.by_service.get_mut(&service_key) {
                handle.menu_path = menu_path.clone();
            }
        }

        upsert_item(&shared, item).await;
        broadcast_state(&shared, &tx, &last_broadcast).await;

        let mut streams = Vec::new();
        if let Ok(item_proxy) = Proxy::new(
            &conn,
            bus_name.clone(),
            item_path.clone(),
            ITEM_INTERFACES[0],
        )
        .await
        {
            for signal_name in ["NewIcon", "NewTitle", "NewToolTip", "NewStatus"] {
                if let Ok(stream) = item_proxy.receive_signal(signal_name).await {
                    streams.push(stream);
                }
            }
        }
        if let Some(menu_path) = &menu_path {
            if let Ok(menu_proxy) = Proxy::new(
                &conn,
                bus_name.clone(),
                menu_path.clone(),
                "com.canonical.dbusmenu",
            )
            .await
            {
                for signal_name in ["LayoutUpdated", "ItemsPropertiesUpdated"] {
                    if let Ok(stream) = menu_proxy.receive_signal(signal_name).await {
                        streams.push(stream);
                    }
                }
            }
        }

        if streams.is_empty() {
            // No signals to watch: the item is static. Park until aborted.
            std::future::pending::<()>().await;
            return;
        }

        let mut combined = futures::stream::select_all(streams);
        if combined.next().await.is_none() {
            // Every signal stream ended (the connection dropped); stop.
            return;
        }
        // A change fired: loop back and rebuild the item from scratch.
    }
}

/// Insert or replace an item in the shared state, keeping items sorted by
/// service key for stable output.
async fn upsert_item(shared: &Arc<Mutex<SystemTrayState>>, item: SystemTrayItem) {
    let mut state = shared.lock().await;
    if let Some(existing) = state
        .items
        .iter_mut()
        .find(|existing| existing.service == item.service)
    {
        *existing = item;
    } else {
        state.items.push(item);
    }
    state
        .items
        .sort_by(|left, right| left.service.cmp(&right.service));
}

/// Serialize the shared state and broadcast it, deduplicating identical
/// consecutive payloads.
async fn broadcast_state(
    shared: &Arc<Mutex<SystemTrayState>>,
    tx: &broadcast::Sender<serde_json::Value>,
    last_broadcast: &Arc<Mutex<Option<serde_json::Value>>>,
) {
    let value = serde_json::to_value(&*shared.lock().await).unwrap_or(serde_json::Value::Null);
    let mut guard = last_broadcast.lock().await;
    if guard.as_ref() == Some(&value) {
        return;
    }
    *guard = Some(value.clone());
    let _ = tx.send(value);
}

/// Build a [`SystemTrayItem`] by reading all of a StatusNotifierItem's
/// properties. Returns the item and the resolved menu object path (if any).
/// Returns `None` when no properties could be read at all.
async fn build_item(
    conn: &Connection,
    bus_name: &str,
    item_path: &str,
    service_key: &str,
) -> Option<(SystemTrayItem, Option<String>)> {
    let proxy = properties_proxy(conn, bus_name, item_path).await.ok()?;
    let properties = read_item_properties(&proxy).await;
    if properties.is_empty() {
        return None;
    }

    let id = string_property(&properties, "Id");
    let title_property = string_property(&properties, "Title");
    let status = {
        let raw = string_property(&properties, "Status");
        if raw.is_empty() {
            "Active".to_string()
        } else {
            raw
        }
    };
    let icon_name = string_property(&properties, "IconName");
    let icon_theme_path = string_property(&properties, "IconThemePath");
    let pixmaps = properties
        .get("IconPixmap")
        .map(parse_pixmaps)
        .unwrap_or_default();
    let tooltip_title = properties
        .get("ToolTip")
        .map(|value| tooltip_title_from_value(value))
        .unwrap_or_default();
    let item_is_menu = bool_property(&properties, "ItemIsMenu");
    let menu_path = menu_path_property(&properties);

    let title = resolve_display_title(&tooltip_title, &title_property, &id);
    let icon = icon::resolve_icon(&icon_name, &icon_theme_path, &pixmaps);
    let menu = match &menu_path {
        Some(path) => fetch_menu(conn, bus_name, path).await,
        None => Vec::new(),
    };

    let item = SystemTrayItem {
        service: service_key.to_string(),
        title: title.clone(),
        tooltip: title,
        status,
        icon,
        item_is_menu,
        menu,
    };
    Some((item, menu_path))
}

/// Apply the title-precedence rule: the tooltip title wins, then the plain
/// `Title`, then the `Id` as a last resort.
fn resolve_display_title(tooltip_title: &str, title: &str, id: &str) -> String {
    if !tooltip_title.is_empty() {
        tooltip_title.to_string()
    } else if !title.is_empty() {
        title.to_string()
    } else {
        id.to_string()
    }
}

/// Fetch and parse a com.canonical.dbusmenu layout into menu nodes. Any
/// failure yields an empty menu rather than failing the whole item.
async fn fetch_menu(conn: &Connection, bus_name: &str, menu_path: &str) -> Vec<SystemTrayMenuNode> {
    let proxy = match Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu").await {
        Ok(proxy) => proxy,
        Err(_) => return Vec::new(),
    };
    let reply: (u32, OwnedValue) = match proxy
        .call("GetLayout", &(0i32, -1i32, &[] as &[&str]))
        .await
    {
        Ok(reply) => reply,
        Err(_) => return Vec::new(),
    };
    menu::parse_menu_layout(&reply.1)
}

/// Build a properties proxy targeting one StatusNotifierItem object.
async fn properties_proxy<'a>(
    conn: &'a Connection,
    bus_name: &str,
    item_path: &str,
) -> zbus::Result<PropertiesProxy<'a>> {
    PropertiesProxy::builder(conn)
        .destination(bus_name.to_string())?
        .path(item_path.to_string())?
        .build()
        .await
}

/// Read all of a StatusNotifierItem's properties, trying the KDE interface
/// name first and falling back to the freedesktop name.
async fn read_item_properties(proxy: &PropertiesProxy<'_>) -> HashMap<String, OwnedValue> {
    for interface in ITEM_INTERFACES {
        let Ok(interface_name) = InterfaceName::try_from(interface) else {
            continue;
        };
        if let Ok(map) = proxy.get_all(Some(interface_name).into()).await {
            if !map.is_empty() {
                return map;
            }
        }
    }
    HashMap::new()
}

/// Strip one level of variant wrapping so downcasts see the concrete value.
fn peel<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => peel(inner),
        other => other,
    }
}

/// Read a string-typed property, defaulting to the empty string.
fn string_property(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties
        .get(key)
        .and_then(|value| String::try_from(&**value).ok())
        .unwrap_or_default()
}

/// Read a boolean-typed property, defaulting to false.
fn bool_property(properties: &HashMap<String, OwnedValue>, key: &str) -> bool {
    properties
        .get(key)
        .and_then(|value| bool::try_from(&**value).ok())
        .unwrap_or(false)
}

/// Read the `Menu` object-path property, returning `None` when it is absent,
/// empty, or the root `/` sentinel (which means "no menu").
fn menu_path_property(properties: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = properties.get("Menu")?;
    let path = match ObjectPath::try_from(&**value) {
        Ok(path) => path.to_string(),
        Err(_) => String::try_from(&**value).ok()?,
    };
    if path.is_empty() || path == "/" {
        None
    } else {
        Some(path)
    }
}

/// Extract the title string (the third field) from a StatusNotifierItem
/// `ToolTip` structure `(sa(iiay)ss)`. Returns the empty string on any
/// mismatch.
fn tooltip_title_from_value(value: &Value<'_>) -> String {
    if let Value::Structure(structure) = peel(value) {
        if let Some(field) = structure.fields().get(2) {
            if let Ok(title) = String::try_from(field) {
                return title;
            }
        }
    }
    String::new()
}

/// Parse an `IconPixmap` value `a(iiay)` into `(width, height, argb_bytes)`
/// triples, skipping any malformed entry.
fn parse_pixmaps(value: &OwnedValue) -> Vec<(i32, i32, Vec<u8>)> {
    let mut pixmaps = Vec::new();
    let Value::Array(array) = peel(value) else {
        return pixmaps;
    };
    for element in array.iter() {
        let Value::Structure(structure) = peel(element) else {
            continue;
        };
        let fields = structure.fields();
        let (Some(width_value), Some(height_value), Some(bytes_value)) =
            (fields.first(), fields.get(1), fields.get(2))
        else {
            continue;
        };
        let (Ok(width), Ok(height)) = (i32::try_from(width_value), i32::try_from(height_value))
        else {
            continue;
        };
        let Value::Array(byte_array) = peel(bytes_value) else {
            continue;
        };
        let bytes: Vec<u8> = byte_array
            .iter()
            .filter_map(|byte| u8::try_from(byte).ok())
            .collect();
        pixmaps.push((width, height, bytes));
    }
    pixmaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_title_wins_over_title_and_id() {
        assert_eq!(
            resolve_display_title("Tooltip Title", "Plain Title", "identifier"),
            "Tooltip Title"
        );
    }

    #[test]
    fn title_is_used_when_tooltip_is_empty() {
        assert_eq!(
            resolve_display_title("", "Plain Title", "identifier"),
            "Plain Title"
        );
    }

    #[test]
    fn id_is_the_last_resort() {
        assert_eq!(resolve_display_title("", "", "identifier"), "identifier");
    }

    #[tokio::test]
    #[ignore = "requires session bus"]
    async fn host_claims_watcher_and_reports_host_registered() {
        let shared = Arc::new(Mutex::new(SystemTrayState::default()));
        let (tx, _rx) = broadcast::channel(16);
        let handles = Arc::new(Mutex::new(ItemHandles::default()));

        let host = tokio::spawn(run_system_tray_host(shared, tx, handles));

        let client = Connection::session().await.expect("session bus");
        let proxy = Proxy::new(&client, WATCHER_BUS_NAME, WATCHER_PATH, WATCHER_BUS_NAME)
            .await
            .expect("watcher proxy");

        let mut registered = false;
        for _ in 0..50 {
            if let Ok(value) = proxy
                .get_property::<bool>("IsStatusNotifierHostRegistered")
                .await
            {
                if value {
                    registered = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        host.abort();
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(
            registered,
            "watcher should report IsStatusNotifierHostRegistered=true"
        );
    }

    /// A minimal fake StatusNotifierItem exporting just enough properties for
    /// the host to build one item.
    struct FakeItem;

    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl FakeItem {
        #[zbus(property)]
        fn id(&self) -> String {
            "fake".to_string()
        }

        #[zbus(property)]
        fn title(&self) -> String {
            "Fake Tray".to_string()
        }

        #[zbus(property)]
        fn status(&self) -> String {
            "Active".to_string()
        }

        #[zbus(property)]
        fn icon_name(&self) -> String {
            "applications-system".to_string()
        }

        #[zbus(property)]
        fn item_is_menu(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    #[ignore = "requires session bus"]
    async fn registering_an_item_broadcasts_it() {
        let shared = Arc::new(Mutex::new(SystemTrayState::default()));
        let (tx, mut rx) = broadcast::channel(64);
        let handles = Arc::new(Mutex::new(ItemHandles::default()));

        let host = tokio::spawn(run_system_tray_host(shared, tx, handles));

        // Wait for the watcher to be ready before serving the fake item.
        let client = Connection::session().await.expect("session bus");
        let watcher = Proxy::new(&client, WATCHER_BUS_NAME, WATCHER_PATH, WATCHER_BUS_NAME)
            .await
            .expect("watcher proxy");
        for _ in 0..50 {
            if watcher
                .get_property::<bool>("IsStatusNotifierHostRegistered")
                .await
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Serve the fake StatusNotifierItem on its own connection.
        let fake_conn = Builder::session()
            .expect("session builder")
            .serve_at("/StatusNotifierItem", FakeItem)
            .expect("serve fake item")
            .build()
            .await
            .expect("fake connection");
        let fake_name = fake_conn
            .unique_name()
            .expect("fake unique name")
            .to_string();

        watcher
            .call_method("RegisterStatusNotifierItem", &(fake_name.as_str(),))
            .await
            .expect("register item");

        let mut saw_item = false;
        for _ in 0..50 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(value)) => {
                    if let Ok(state) = serde_json::from_value::<SystemTrayState>(value.clone()) {
                        if state.items.iter().any(|item| item.title == "Fake Tray") {
                            saw_item = true;
                            break;
                        }
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => continue,
            }
        }

        host.abort();
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(saw_item, "a broadcast should contain the fake tray item");
    }
}
