#![cfg(feature = "gtk-test")]

use gtk4::prelude::*;
use quantum_ui::web_process::{apply_widget_settings, build_web_context};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use webkit6::prelude::*;

const PAGE_URI: &str = "quantum://plugin/file-viewer/views/file-viewer/index.html";
const CONTROL_URI: &str = "quantum://plugin/file-viewer/views/file-viewer/control.svg";
const BUNDLE_PREFIX: &str = "quantum://plugin/file-viewer/views/file-viewer/";
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="17" height="11"><rect width="17" height="11" fill="red"/></svg>"#;
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5, 1, 1,
    39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct FixtureDispatcher {
    responses: HashMap<String, Value>,
    delayed_entered: Arc<std::sync::atomic::AtomicBool>,
    delayed_returned: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl quantum_ui::IpcDispatcher for FixtureDispatcher {
    async fn dispatch(
        &self,
        method: &str,
        params: Value,
    ) -> quantum_ui::dispatcher::DispatchResult {
        assert!(matches!(
            method,
            "file-viewer.read" | "launcher.search" | "view.hide"
        ));
        if params["delayed"] == json!(true) {
            self.delayed_entered
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.release.notified().await;
            self.delayed_returned
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if method != "file-viewer.read" {
            if params["fail"] == json!(true) {
                return Err(quantum_ui::dispatcher::DispatchError {
                    code: -32000,
                    message: "ordinary fixture failure".to_string(),
                });
            }
            return Ok(json!({"method": method, "settled": true}));
        }
        self.responses
            .get(params["path"].as_str().unwrap())
            .cloned()
            .ok_or_else(|| quantum_ui::dispatcher::DispatchError {
                code: -32000,
                message: "fixture not found".to_string(),
            })
    }
}

fn read_through_bridge(view: &webkit6::WebView, path: &std::path::Path, identity: u64) -> Value {
    let message = json!({"id": identity, "method": "file-viewer.read", "params": {"path": path}});
    evaluate(
        view,
        &format!(
            "window.webkit.messageHandlers.quantum.postMessage({}); JSON.stringify(null)",
            serde_json::to_string(&message.to_string()).unwrap()
        ),
    );
    pump_until("image response through real bridge", || {
        evaluate(view, &format!("JSON.stringify(Object.hasOwn(window.readResponses, {identity}) || Object.hasOwn(window.readErrors, {identity}))")) == json!(true)
    });
    let result = evaluate(view, &format!("JSON.stringify({{response: window.readResponses[{identity}], error: window.readErrors[{identity}]}})"));
    assert!(result.get("error").is_none(), "bridge error: {result}");
    result["response"].clone()
}

fn initialize_bridge_client(view: &webkit6::WebView) {
    evaluate(
        view,
        r#"
window.readResponses = {}; window.readErrors = {}; window.readSettlements = {};
window.pendingReads = {};
window.__quantum_resolve = (identity, response) => {
    window.readResponses[identity] = response;
    window.pendingReads[identity]?.resolve(response);
    delete window.pendingReads[identity];
};
window.__quantum_reject = (identity, error) => {
    window.readErrors[identity] = error;
    window.pendingReads[identity]?.reject(error);
    delete window.pendingReads[identity];
};
window.startRead = (identity, path, delayed, method = 'file-viewer.read', fail = false) => {
    const promise = new Promise((resolve, reject) => {
        window.pendingReads[identity] = {resolve, reject};
        window.webkit.messageHandlers.quantum.postMessage(JSON.stringify({
            id: identity, method, params: {path, delayed, fail}
        }));
    });
    promise.then(() => window.readSettlements[identity] = 'resolved',
        () => window.readSettlements[identity] = 'rejected');
};
JSON.stringify(null)
"#,
    );
}

fn probe_image(view: &webkit6::WebView, name: &str, uri: &str) -> Value {
    let name = serde_json::to_string(name).unwrap();
    evaluate(
        view,
        &format!(
            "addImage({name}, {}); JSON.stringify(null)",
            serde_json::to_string(uri).unwrap()
        ),
    );
    pump_until("image load or error event", || {
        evaluate(
            view,
            &format!("JSON.stringify(window.imageEvents[{name}].length > 0)"),
        ) == json!(true)
    });
    let observation = evaluate(
        view,
        &format!(
            r#"JSON.stringify((() => {{
        const image = document.getElementById({name});
        return {{loaded: image.naturalWidth > 0, naturalWidth: image.naturalWidth,
            naturalHeight: image.naturalHeight, events: window.imageEvents[{name}], source: image.currentSrc}};
    }})())"#
        ),
    );
    eprintln!("secure probe name={name} observation={observation}");
    observation
}

fn pump_until(label: &str, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let context = glib::MainContext::default();
    while !ready() {
        assert!(Instant::now() < deadline, "deadline expired: {label}");
        // A bounded dispatch batch also bounds continuously ready sources.
        for _ in 0..32 {
            if !context.pending() {
                break;
            }
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn evaluate(view: &webkit6::WebView, script: &str) -> Value {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    view.evaluate_javascript(
        script,
        None,
        None,
        None::<&gio::Cancellable>,
        move |value| {
            *callback_result.borrow_mut() = Some(value.map(|value| value.to_str().to_string()));
        },
    );
    pump_until("asynchronous JavaScript callback", || {
        result.borrow().is_some()
    });
    let text = result
        .borrow_mut()
        .take()
        .unwrap()
        .expect("JavaScript evaluation failed");
    serde_json::from_str(&text).expect("JavaScript must return JSON.stringify output")
}

fn page(file_uri: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head><body>
<script>
window.imageEvents = {{}};
function addImage(name, source) {{
    const image = new Image();
    image.id = name;
    window.imageEvents[name] = [];
    image.onload = () => window.imageEvents[name].push('load');
    image.onerror = () => window.imageEvents[name].push('error');
    image.src = source;
    document.body.append(image);
}}
addImage('fileImage', {file_source});
addImage('schemeControl', {control_source});
</script></body></html>"#,
        file_source = serde_json::to_string(file_uri).unwrap(),
        control_source = serde_json::to_string(CONTROL_URI).unwrap(),
    )
}

fn snapshot(view: &webkit6::WebView) -> Value {
    evaluate(
        view,
        r#"JSON.stringify({page: location.href, origin: location.origin,
documentOrigin: document.location.origin, readyState: document.readyState,
images: Object.fromEntries(['fileImage', 'schemeControl'].map(name => {
    const image = document.getElementById(name);
    if (!image) return [name, {missing: true}];
    return [name, {source: image.currentSrc, complete: image.complete,
        naturalWidth: image.naturalWidth, naturalHeight: image.naturalHeight,
        loaded: image.naturalWidth > 0, events: window.imageEvents[name]}];
}))})"#,
    )
}

#[test]
#[ignore = "requires an isolated Xvfb and private DBus session; never run on the desktop"]
fn native_file_image_transport_and_request_view_identity() {
    assert_eq!(
        std::env::var("QUANTUM_NATIVE_TEST_ISOLATED").as_deref(),
        Ok("1")
    );
    assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("x11"));
    assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    assert!(std::env::var_os("WAYLAND_SOCKET").is_none());
    assert!(std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").is_none());
    gtk4::init().expect("isolated GTK initialization failed");
    let directory = tempfile::tempdir().unwrap();
    let image_path = directory.path().join("image.svg");
    std::fs::write(&image_path, SVG).unwrap();
    let file_uri = gio::File::for_path(&image_path).uri().to_string();
    assert_eq!(file_uri, format!("file://{}", image_path.display()));
    let html = page(&file_uri);
    let file_page_path = directory.path().join("index.html");
    std::fs::write(&file_page_path, &html).unwrap();
    let file_page_uri = gio::File::for_path(&file_page_path).uri().to_string();

    let context = build_web_context();
    let bundle = std::env::var_os("QUANTUM_NATIVE_VIEWER_DIST")
        .filter(|path| !path.is_empty())
        .map(|path| Rc::new(load_built_bundle(Path::new(&path))));
    let bundle_viewers = Rc::new(RefCell::new(HashSet::new()));
    let callback_bundle = bundle.clone();
    let callback_bundle_viewers = bundle_viewers.clone();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let callback_requests = requests.clone();
    context.register_uri_scheme("quantum", move |request| {
        let uri = request.uri().expect("scheme request URI").to_string();
        let identity = request.web_view().map(|view| view.as_ptr() as usize);
        eprintln!("scheme request uri={uri} web_view={identity:?}");
        callback_requests.borrow_mut().push((uri.clone(), identity));
        if quantum_ui::viewer_image_resources::serve_request(request) {
            return;
        }
        if identity.is_some_and(|identity| callback_bundle_viewers.borrow().contains(&identity)) {
            if let Some((bytes, content_type)) =
                callback_bundle.as_ref().and_then(|bundle| bundle.get(&uri))
            {
                eprintln!(
                    "built bundle resource uri={uri} bytes={} content_type={content_type}",
                    bytes.len()
                );
                let stream =
                    gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes.clone()));
                request.finish(&stream, bytes.len() as i64, Some(content_type));
            } else {
                request.finish_error(&mut glib::Error::new(
                    glib::FileError::Noent,
                    "bundle resource not found",
                ));
            }
            return;
        }
        let (content, content_type) = match uri.as_str() {
            PAGE_URI => (html.as_bytes(), "text/html"),
            CONTROL_URI => (SVG.as_bytes(), "image/svg+xml"),
            _ => {
                request.finish_error(&mut glib::Error::new(glib::FileError::Noent, &uri));
                return;
            }
        };
        let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(content.to_vec()));
        request.finish(&stream, content.len() as i64, Some(content_type));
    });

    let failures = Rc::new(RefCell::new(Vec::new()));
    let terminated = Rc::new(Cell::new(0));
    // The global default finalizes on the process main thread, not this test's
    // GTK thread. Own an otherwise default session so teardown stays here.
    let session = webkit6::NetworkSession::new(None, None);
    let views: Vec<_> = (0..3)
        .map(|_| {
            let view = webkit6::WebView::builder()
                .web_context(&context)
                .network_session(&session)
                .build();
            let settings = webkit6::Settings::new();
            apply_widget_settings(&settings);
            view.set_settings(&settings);
            let load_failures = failures.clone();
            view.connect_load_failed(move |_, event, uri, error| {
                load_failures
                    .borrow_mut()
                    .push(format!("WebKit load failure: {event:?} {uri}: {error}"));
                false
            });
            let process_failures = failures.clone();
            let process_terminated = terminated.clone();
            view.connect_web_process_terminated(move |_, reason| {
                if reason == webkit6::WebProcessTerminationReason::TerminatedByApi {
                    process_terminated.set(process_terminated.get() + 1);
                    return;
                }
                process_failures
                    .borrow_mut()
                    .push(format!("WebKit process terminated: {reason:?}"));
            });
            view
        })
        .collect();
    let finished: Vec<_> = views
        .iter()
        .map(|view| {
            let finished = Rc::new(Cell::new(false));
            let callback_finished = finished.clone();
            view.connect_load_changed(move |_, event| {
                if event == webkit6::LoadEvent::Finished {
                    callback_finished.set(true);
                }
            });
            finished
        })
        .collect();
    views[0].load_uri(PAGE_URI);
    views[1].load_uri(PAGE_URI);
    views[2].load_uri(&file_page_uri);
    pump_until("three bare WebViews finish loading", || {
        !failures.borrow().is_empty() || finished.iter().all(|state| state.get())
    });
    assert!(
        failures.borrow().is_empty(),
        "infrastructure failures: {:?}",
        failures.borrow()
    );

    let observations: Vec<_> = views.iter().map(snapshot).collect();
    for (viewer, observation) in observations.iter().enumerate() {
        eprintln!(
            "viewer={viewer} observation={}",
            serde_json::to_string_pretty(observation).unwrap()
        );
        assert_eq!(
            observation["images"]["schemeControl"]["loaded"],
            json!(true),
            "same-scheme positive control"
        );
        assert_eq!(
            observation["images"]["schemeControl"]["naturalWidth"],
            json!(17)
        );
        assert_eq!(
            observation["images"]["schemeControl"]["naturalHeight"],
            json!(11)
        );
        assert_eq!(
            observation["images"]["schemeControl"]["events"],
            json!(["load"])
        );
    }
    assert_eq!(
        observations[2]["images"]["fileImage"]["loaded"],
        json!(true),
        "same file must decode from a file page"
    );
    assert_eq!(
        observations[2]["images"]["fileImage"]["naturalWidth"],
        json!(17)
    );
    assert_eq!(
        observations[2]["images"]["fileImage"]["naturalHeight"],
        json!(11)
    );
    assert_eq!(
        observations[2]["images"]["fileImage"]["events"],
        json!(["load"])
    );
    assert_eq!(observations[2]["page"], json!(file_page_uri));
    assert_eq!(observations[2]["origin"], json!("file://"));
    let first_identity = views[0].as_ptr() as usize;
    let second_identity = views[1].as_ptr() as usize;
    assert_ne!(first_identity, second_identity);
    for identity in [first_identity, second_identity] {
        for uri in [PAGE_URI, CONTROL_URI] {
            assert!(
                requests
                    .borrow()
                    .contains(&(uri.to_string(), Some(identity))),
                "request {uri} must identify viewer {identity}"
            );
        }
    }
    assert!(requests
        .borrow()
        .iter()
        .all(|(_, identity)| identity.is_some()));
    eprintln!("distinct request identities verified: {first_identity} and {second_identity}, same context and page URI");
    for observation in &observations[..2] {
        assert_eq!(observation["page"], json!(PAGE_URI));
        assert_eq!(observation["origin"], json!("quantum://plugin"));
        // Characterize the existing cross-scheme failure, not a transport fix.
        assert_eq!(
            observation["images"]["fileImage"]["loaded"],
            json!(false),
            "quantum page rejects file image transport"
        );
        assert_eq!(observation["images"]["fileImage"]["naturalWidth"], json!(0));
        assert_eq!(
            observation["images"]["fileImage"]["naturalHeight"],
            json!(0)
        );
        assert_eq!(
            observation["images"]["fileImage"]["events"],
            json!(["error"])
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut responses = HashMap::new();
    let mut fixtures = Vec::new();
    for (filename, bytes, mime_type, width, height) in [
        (
            "space # %2F \u{00e9}.svg",
            SVG.as_bytes(),
            "image/svg+xml",
            17,
            11,
        ),
        ("space # %2F \u{00e9}.png", PNG, "image/png", 1, 1),
    ] {
        let path = directory.path().join(filename);
        std::fs::write(&path, bytes).unwrap();
        responses.insert(
            path.to_str().unwrap().to_string(),
            json!({
                "file_type": "image", "directory": directory.path(), "filename": filename,
                "mime_type": mime_type, "uri": format!("file://{}", path.display()),
                "content": "", "size": bytes.len(),
            }),
        );
        fixtures.push((path, width, height));
    }
    let delayed_entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let delayed_returned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(tokio::sync::Notify::new());
    let text_path = directory.path().join("text.txt");
    std::fs::write(&text_path, "routine text").unwrap();
    responses.insert(
        text_path.to_str().unwrap().to_string(),
        json!({
            "file_type": "text", "content": "routine text", "filename": "text.txt",
            "directory": directory.path(), "size": 12,
        }),
    );
    let dispatcher = Arc::new(FixtureDispatcher {
        responses,
        delayed_entered: delayed_entered.clone(),
        delayed_returned: delayed_returned.clone(),
        release: release.clone(),
    });
    for view in &views[..2] {
        quantum_ui::register_bridge(
            view,
            dispatcher.clone(),
            runtime.handle().clone(),
            Arc::new(std::sync::Mutex::new(None)),
        );
        initialize_bridge_client(view);
    }
    for (identity, method, path, reopen, fail, expected) in [
        (600, "launcher.search", &text_path, false, false, "resolved"),
        (607, "launcher.search", &text_path, true, false, "resolved"),
        (601, "launcher.search", &text_path, true, true, "rejected"),
        (602, "view.hide", &text_path, false, false, "resolved"),
        (
            603,
            "file-viewer.read",
            &fixtures[0].0,
            false,
            false,
            "rejected",
        ),
        (604, "file-viewer.read", &text_path, true, false, "rejected"),
        (
            605,
            "file-viewer.read",
            &fixtures[0].0,
            true,
            false,
            "rejected",
        ),
    ] {
        delayed_entered.store(false, std::sync::atomic::Ordering::SeqCst);
        delayed_returned.store(false, std::sync::atomic::Ordering::SeqCst);
        evaluate(
            &views[0],
            &format!(
                "window.startRead({identity}, {}, true, {}, {fail}); JSON.stringify(null)",
                serde_json::to_string(path).unwrap(),
                serde_json::to_string(method).unwrap()
            ),
        );
        pump_until("warm-view request is in flight", || {
            delayed_entered.load(std::sync::atomic::Ordering::SeqCst)
        });
        quantum_ui::viewer_image_resources::suspend_view(&views[0], false);
        if reopen {
            quantum_ui::viewer_image_resources::activate_view(&views[0]);
        }
        release.notify_one();
        pump_until(
            "warm hide must settle the retained document's promise",
            || {
                evaluate(
                    &views[0],
                    &format!("JSON.stringify(window.readSettlements[{identity}] === '{expected}')"),
                ) == json!(true)
            },
        );
        assert_eq!(
            evaluate(
                &views[0],
                "JSON.stringify(Object.keys(window.pendingReads).length)"
            ),
            json!(0)
        );
        if method == "file-viewer.read" {
            assert_eq!(evaluate(&views[0], &format!("JSON.stringify({{code: window.readErrors[{identity}].code, resolved: Object.hasOwn(window.readResponses, {identity})}})")), json!({"code": -32800, "resolved": false}));
        } else if fail {
            assert_eq!(
                evaluate(
                    &views[0],
                    &format!("JSON.stringify(window.readErrors[{identity}].code)")
                ),
                json!(-32000)
            );
        } else {
            assert_eq!(
                evaluate(
                    &views[0],
                    &format!("JSON.stringify(window.readResponses[{identity}])")
                ),
                json!({"method": method, "settled": true})
            );
        }
        quantum_ui::viewer_image_resources::activate_view(&views[0]);
        eprintln!("warm hide method={method} reopen={reopen} fail={fail}: {expected}, no retained promises");
    }
    quantum_ui::viewer_image_resources::suspend_view(&views[0], false);
    evaluate(
        &views[0],
        &format!(
            "window.startRead(606, {}, false); JSON.stringify(null)",
            serde_json::to_string(&fixtures[0].0).unwrap()
        ),
    );
    pump_until(
        "new read while hidden rejects rather than retaining a promise",
        || {
            evaluate(
                &views[0],
                "JSON.stringify(window.readSettlements[606] === 'rejected')",
            ) == json!(true)
        },
    );
    assert_eq!(
        evaluate(&views[0], "JSON.stringify(window.readErrors[606].code)"),
        json!(-32800)
    );
    quantum_ui::viewer_image_resources::activate_view(&views[0]);
    for (case, old_path, latest_path) in [
        (0, &text_path, &text_path),
        (1, &fixtures[0].0, &fixtures[1].0),
    ] {
        delayed_entered.store(false, std::sync::atomic::Ordering::SeqCst);
        delayed_returned.store(false, std::sync::atomic::Ordering::SeqCst);
        let old_identity = 500 + case * 2;
        let latest_identity = old_identity + 1;
        evaluate(
            &views[0],
            &format!(
                "window.startRead({old_identity}, {}, true); JSON.stringify(null)",
                serde_json::to_string(old_path).unwrap()
            ),
        );
        pump_until("older read is genuinely in flight", || {
            delayed_entered.load(std::sync::atomic::Ordering::SeqCst)
        });
        evaluate(
            &views[0],
            &format!(
                "window.startRead({latest_identity}, {}, false); JSON.stringify(null)",
                serde_json::to_string(latest_path).unwrap()
            ),
        );
        pump_until("newer read resolves", || {
            evaluate(
                &views[0],
                &format!(
                    "JSON.stringify(window.readSettlements[{latest_identity}] === 'resolved')"
                ),
            ) == json!(true)
        });
        release.notify_one();
        pump_until("both overlapping read promises settle", || {
            evaluate(&views[0], &format!("JSON.stringify(window.readSettlements[{old_identity}] === 'rejected' && window.readSettlements[{latest_identity}] === 'resolved')")) == json!(true)
        });
        assert_eq!(
            evaluate(
                &views[0],
                &format!("JSON.stringify(window.readErrors[{old_identity}].code)")
            ),
            json!(-32800)
        );
        assert_eq!(
            evaluate(
                &views[0],
                "JSON.stringify(Object.keys(window.pendingReads).length)"
            ),
            json!(0)
        );
        eprintln!(
            "overlapping reads case={case}: old rejected, new resolved, no retained promises"
        );
    }
    for (fixture, (path, width, height)) in fixtures.iter().enumerate() {
        let response = read_through_bridge(&views[0], path, fixture as u64 + 100);
        let uri = response["uri"].as_str().unwrap();
        assert!(
            uri.starts_with("quantum://viewer-image/"),
            "bridge must replace raw file transport: {response}"
        );
        assert!(!uri.contains(path.to_str().unwrap()));
        let observation = probe_image(&views[0], &format!("secure{fixture}"), uri);
        assert_eq!(observation["loaded"], json!(true));
        assert_eq!(observation["naturalWidth"], json!(width));
        assert_eq!(observation["naturalHeight"], json!(height));
        assert_eq!(
            probe_image(&views[1], &format!("foreign{fixture}"), uri)["loaded"],
            json!(false)
        );
        assert_eq!(
            probe_image(&views[0], &format!("repeat{fixture}"), uri)["loaded"],
            json!(true)
        );
        quantum_ui::viewer_image_resources::suspend_view(&views[0], false);
        assert_eq!(
            probe_image(&views[0], &format!("revoked{fixture}"), uri)["loaded"],
            json!(false)
        );
        quantum_ui::viewer_image_resources::activate_view(&views[0]);
    }
    assert_eq!(
        probe_image(
            &views[0],
            "unissued",
            &format!("quantum://viewer-image/{}", "0".repeat(64))
        )["loaded"],
        json!(false)
    );
    assert_eq!(
        probe_image(
            &views[0],
            "arbitraryPath",
            "quantum://viewer-image/%2Ftmp%2Funopened.svg"
        )["loaded"],
        json!(false)
    );

    let response = read_through_bridge(&views[0], &fixtures[0].0, 200);
    let previous_uri = response["uri"].as_str().unwrap();
    let latest = read_through_bridge(&views[0], &fixtures[1].0, 201);
    assert_eq!(
        probe_image(&views[0], "afterNewRead", previous_uri)["loaded"],
        json!(false)
    );
    assert_eq!(
        probe_image(&views[0], "latestRead", latest["uri"].as_str().unwrap())["loaded"],
        json!(true)
    );
    let response = read_through_bridge(&views[0], &fixtures[0].0, 202);
    let navigation_uri = response["uri"].as_str().unwrap();
    finished[0].set(false);
    views[0].load_uri(PAGE_URI);
    pump_until("navigation revokes image grant", || finished[0].get());
    assert_eq!(
        probe_image(&views[0], "afterNavigation", navigation_uri)["loaded"],
        json!(false)
    );

    initialize_bridge_client(&views[0]);
    delayed_entered.store(false, std::sync::atomic::Ordering::SeqCst);
    delayed_returned.store(false, std::sync::atomic::Ordering::SeqCst);
    evaluate(
        &views[0],
        &format!(
            "window.startRead(290, {}, true); JSON.stringify(null)",
            serde_json::to_string(&fixtures[0].0).unwrap()
        ),
    );
    pump_until("old document read is in flight", || {
        delayed_entered.load(std::sync::atomic::Ordering::SeqCst)
    });
    finished[0].set(false);
    views[0].load_uri(PAGE_URI);
    pump_until("new document replaces pending caller", || finished[0].get());
    initialize_bridge_client(&views[0]);
    evaluate(
        &views[0],
        &format!(
            "window.startRead(290, {}, false); JSON.stringify(null)",
            serde_json::to_string(&text_path).unwrap()
        ),
    );
    pump_until("new document reuses request identity", || {
        evaluate(
            &views[0],
            "JSON.stringify(window.readSettlements[290] === 'resolved')",
        ) == json!(true)
    });
    release.notify_one();
    pump_until("old document dispatcher returns", || {
        delayed_returned.load(std::sync::atomic::Ordering::SeqCst)
    });
    let navigation_settle_deadline = Instant::now() + Duration::from_millis(500);
    pump_until("pump old document response", || {
        Instant::now() >= navigation_settle_deadline
    });
    assert_eq!(evaluate(&views[0], "JSON.stringify({content: window.readResponses[290].content, rejected: Object.hasOwn(window.readErrors, 290), pending: Object.keys(window.pendingReads).length})"), json!({"content": "routine text", "rejected": false, "pending": 0}));
    eprintln!(
        "navigation: old delayed response cannot overwrite or reject the reused request identity"
    );

    initialize_bridge_client(&views[0]);
    delayed_entered.store(false, std::sync::atomic::Ordering::SeqCst);
    delayed_returned.store(false, std::sync::atomic::Ordering::SeqCst);
    let pending = json!({"id": 300, "method": "file-viewer.read", "params": {"path": fixtures[0].0, "delayed": true}});
    evaluate(
        &views[0],
        &format!(
            "window.webkit.messageHandlers.quantum.postMessage({}); JSON.stringify(null)",
            serde_json::to_string(&pending.to_string()).unwrap()
        ),
    );
    pump_until("dispatcher is genuinely in flight", || {
        delayed_entered.load(std::sync::atomic::Ordering::SeqCst)
    });
    quantum_ui::viewer_image_resources::suspend_view(&views[0], true);
    quantum_ui::viewer_image_resources::activate_view(&views[0]);
    release.notify_one();
    pump_until("closed viewer's dispatcher response returns", || {
        delayed_returned.load(std::sync::atomic::Ordering::SeqCst)
    });
    let settle_deadline = Instant::now() + Duration::from_millis(500);
    pump_until("pump late response after disposal", || {
        Instant::now() >= settle_deadline
    });
    assert_eq!(evaluate(&views[0], "JSON.stringify(Object.hasOwn(window.readResponses, 300) || Object.hasOwn(window.readErrors, 300))"), json!(false));
    eprintln!("secure transport: PNG and SVG decode; foreign, revoked, unissued, arbitrary path, new-read and navigation requests rejected; late closed-view response suppressed");
    if bundle.is_some() {
        verify_built_viewer(&context, &session, &runtime, &bundle_viewers);
    } else {
        eprintln!("built viewer checks not requested; set QUANTUM_NATIVE_VIEWER_DIST to the built dist directory");
    }
    for view in &views {
        view.terminate_web_process();
    }
    pump_until("all harness-owned render processes terminate", || {
        terminated.get() == views.len()
    });
    assert!(
        failures.borrow().is_empty(),
        "infrastructure failures: {:?}",
        failures.borrow()
    );
}

// Preload a closed map of bundle resources. The scheme callback never turns a
// request path into a filesystem read, including requests for unknown assets.
fn load_built_bundle(root: &Path) -> HashMap<String, (Vec<u8>, &'static str)> {
    let root = root.canonicalize().expect("built bundle root exists");
    let mut pending = vec![root.clone()];
    let mut resources = HashMap::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            assert!(
                !entry.file_type().unwrap().is_symlink(),
                "bundle must not contain symlinks: {}",
                path.display()
            );
            assert!(path.canonicalize().unwrap().starts_with(&root));
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path.strip_prefix(&root).unwrap().to_str().unwrap();
            let content_type = match path.extension().and_then(|extension| extension.to_str()) {
                Some("html") => "text/html",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("wasm") => "application/wasm",
                Some("json") => "application/json",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("woff2") => "font/woff2",
                Some("woff") => "font/woff",
                _ => continue,
            };
            let mut bytes = std::fs::read(&path).unwrap();
            let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha256).unwrap();
            checksum.update(&bytes);
            if relative == "index.html" || relative.starts_with("assets/index-") {
                eprintln!(
                    "built bundle source={} sha256={}",
                    path.display(),
                    checksum.string().unwrap()
                );
            }
            if relative == "index.html" {
                let tokens = [
                    ("font-mono", "monospace"),
                    ("font-sans", "sans-serif"),
                    ("color-bg", "#ffffff"),
                    ("color-bg-alt", "#f4f4f4"),
                    ("color-fg", "#111111"),
                    ("color-fg-alt", "#333333"),
                    ("color-border", "#cccccc"),
                    ("color-muted", "#666666"),
                    ("color-accent", "#2244bb"),
                    ("color-error", "#bb2222"),
                ]
                .into_iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
                let html = quantum_ui::scheme::inject_tokens(
                    std::str::from_utf8(&bytes).unwrap(),
                    &tokens,
                );
                bytes = quantum_ui::scheme::inject_plugin_client(&html).into_bytes();
            }
            resources.insert(format!("{BUNDLE_PREFIX}{relative}"), (bytes, content_type));
        }
    }
    assert!(resources.contains_key(PAGE_URI), "bundle has no index.html");
    eprintln!(
        "built bundle snapshot root={} resources={}",
        root.display(),
        resources.len()
    );
    resources
}

struct BuiltFixtureDispatcher {
    responses: HashMap<String, Value>,
    calls: std::sync::Mutex<Vec<(String, Value)>>,
}

#[async_trait::async_trait]
impl quantum_ui::IpcDispatcher for BuiltFixtureDispatcher {
    async fn dispatch(
        &self,
        method: &str,
        params: Value,
    ) -> quantum_ui::dispatcher::DispatchResult {
        self.calls
            .lock()
            .unwrap()
            .push((method.to_string(), params.clone()));
        if method == "view.hide" {
            return Ok(Value::Null);
        }
        if method == "file-viewer.read" {
            if let Some(response) = params["path"]
                .as_str()
                .and_then(|path| self.responses.get(path))
            {
                return Ok(response.clone());
            }
        }
        Err(quantum_ui::dispatcher::DispatchError {
            code: -32000,
            message: "request outside trusted fixture set".to_string(),
        })
    }
}

fn wait_for_dom(view: &webkit6::WebView, label: &str, predicate: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while evaluate(view, &format!("JSON.stringify(Boolean({predicate}))")) != json!(true) {
        if Instant::now() >= deadline {
            let diagnostic = evaluate(view, "JSON.stringify({page: location.href, body: document.body?.innerText, errors: window.nativeErrors})");
            panic!("built viewer deadline expired: {label}: {diagnostic}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn load_built_case(view: &webkit6::WebView, path: &Path) {
    let manager = view.user_content_manager().unwrap();
    manager.remove_all_scripts();
    // inject_view_args is crate-private. Supply its contract at document start,
    // before the real App and client install their own callbacks; do not mock them.
    let script = format!(
        r#"
        window.__quantum_args = {{path: {path}}};
        window.__quantum_view_name = 'plugin/file-viewer/file-viewer#native-bundle';
        window.nativeErrors = [];
        window.addEventListener('error', event => window.nativeErrors.push(String(event.message)));
        window.addEventListener('unhandledrejection', event => window.nativeErrors.push(String(event.reason)));
    "#,
        path = serde_json::to_string(path).unwrap()
    );
    manager.add_script(&webkit6::UserScript::new(
        &script,
        webkit6::UserContentInjectedFrames::TopFrame,
        webkit6::UserScriptInjectionTime::Start,
        &[],
        &[],
    ));
    view.load_uri(PAGE_URI);
    let filename = serde_json::to_string(path.file_name().unwrap().to_str().unwrap()).unwrap();
    wait_for_dom(view, "real App loaded trusted fixture", &format!("document.querySelector('header .filename')?.textContent === {filename} && !document.querySelector('.loading')"));
}

fn press_viewer_key(view: &webkit6::WebView, key: &str, control: bool, shift: bool) {
    let options = json!({"key": key, "ctrlKey": control, "shiftKey": shift, "bubbles": true, "cancelable": true});
    evaluate(view, &format!("document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {options})); JSON.stringify(null)"));
}

fn search_built_viewer(view: &webkit6::WebView, query: &str, indicator: &str) {
    press_viewer_key(view, "f", true, false);
    wait_for_dom(
        view,
        "Ctrl+F opens and focuses real SearchBar",
        "document.activeElement?.classList.contains('search-input')",
    );
    evaluate(view, &format!("document.activeElement.value = {}; document.activeElement.dispatchEvent(new Event('input', {{bubbles: true}})); JSON.stringify(null)", serde_json::to_string(query).unwrap()));
    wait_for_dom(
        view,
        "real search count",
        &format!(
            "document.querySelector('.match-indicator')?.textContent === {}",
            serde_json::to_string(indicator).unwrap()
        ),
    );
}

fn current_search_state(view: &webkit6::WebView, viewport: &str) -> Value {
    evaluate(view, "window.nativeLayoutSettled = false; requestAnimationFrame(() => requestAnimationFrame(() => window.nativeLayoutSettled = true)); JSON.stringify(null)");
    wait_for_dom(
        view,
        "layout settles for viewport measurement",
        "window.nativeLayoutSettled",
    );
    evaluate(
        view,
        &format!(
            r#"JSON.stringify((() => {{
        const current = document.querySelector('.search-match-current');
        const viewport = document.querySelector({viewport});
        const bounds = current?.getBoundingClientRect();
        const viewportBounds = viewport.getBoundingClientRect();
        const rows = viewport.querySelectorAll('.text-line');
        return {{indicator: document.querySelector('.match-indicator')?.textContent,
            text: current?.textContent, line: current?.closest('[data-line]')?.dataset.line,
            scrollTop: viewport.scrollTop, clientHeight: viewport.clientHeight,
            currentBounds: bounds ? {{top: bounds.top, bottom: bounds.bottom, height: bounds.height}} : null,
            viewportBounds: {{top: viewportBounds.top, bottom: viewportBounds.bottom}},
            lineHeight: current ? getComputedStyle(current).lineHeight : null,
            rowSpacing: rows.length > 1 ? rows[1].getBoundingClientRect().top - rows[0].getBoundingClientRect().top : null,
            visible: Boolean(bounds && bounds.top >= viewportBounds.top && bounds.bottom <= viewportBounds.bottom)}};
    }})())"#,
            viewport = serde_json::to_string(viewport).unwrap()
        ),
    )
}

fn verify_built_viewer(
    context: &webkit6::WebContext,
    session: &webkit6::NetworkSession,
    runtime: &tokio::runtime::Runtime,
    bundle_viewers: &Rc<RefCell<HashSet<usize>>>,
) {
    let mut findings = Vec::new();
    let directory = tempfile::tempdir_in("/tmp/opencode").unwrap();
    let mut responses = HashMap::new();
    let mut image_cases = Vec::new();
    for (extension, content_type, bytes, width, height) in [
        ("svg", "image/svg+xml", SVG.as_bytes().to_vec(), 17, 11),
        ("png", "image/png", PNG.to_vec(), 1, 1),
    ] {
        let path = directory
            .path()
            .join(format!("native space # %2F \u{00e9}.{extension}"));
        std::fs::write(&path, &bytes).unwrap();
        image_cases.push((path, content_type, width, height, true));
    }
    for (extension, content_type) in [
        ("jpg", "image/jpeg"),
        ("gif", "image/gif"),
        ("webp", "image/webp"),
        ("bmp", "image/bmp"),
        ("ico", "image/x-icon"),
        ("avif", "image/avif"),
    ] {
        let fixture = std::env::var_os("QUANTUM_NATIVE_IMAGE_FIXTURES")
            .filter(|path| !path.is_empty())
            .map(|root| PathBuf::from(root).join(format!("fixture.{extension}")));
        if let Some(fixture) = fixture.filter(|path| path.is_file()) {
            let path = directory
                .path()
                .join(format!("native space # %2F \u{00e9}.{extension}"));
            std::fs::copy(fixture, &path).unwrap();
            image_cases.push((path, content_type, 16, 16, false));
        } else {
            eprintln!("built viewer format={extension} UNAVAILABLE: no generated fixture supplied");
        }
    }
    let broken_path = directory.path().join("deliberately-corrupt.png");
    std::fs::write(&broken_path, b"not an image").unwrap();
    image_cases.push((broken_path, "image/png", 0, 0, false));
    for (path, content_type, _, _, _) in &image_cases {
        responses.insert(path.to_str().unwrap().to_string(), json!({"file_type": "image", "content": "",
            "directory": directory.path(), "filename": path.file_name().unwrap().to_str().unwrap(),
            "uri": format!("file://{}", path.display()), "mime_type": content_type, "size": std::fs::metadata(path).unwrap().len()}));
    }
    let text = (0..1000)
        .map(|line| {
            if line == 10 || line == 900 {
                format!("line {line}: needle")
            } else {
                format!("line {line}: ordinary content")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let json_content = "{\n  \"target\": {\n    \"nested\": {\n      \"value\": \"needle folded\"\n    }\n  },\n  \"sibling\": {\n    \"value\": \"untouched\"\n  },\n  \"tail\": \"needle tail\"\n}";
    let markdown = format!("# Native Search\n\nFirst visible **needle** inside emphasis.\n\n{}\n\nLast visible *needle* inside emphasis.\n\n```mermaid\ngraph TD\n A[needle diagram] --> B[other]\n```\n", "Ordinary paragraph to create vertical scrolling.\n\n".repeat(150));
    let mut text_cases = Vec::new();
    for (filename, file_type, content) in [
        ("thousand-lines.txt", "text", text.as_str()),
        ("nested-folds.json", "json", json_content),
        ("rendered-text.md", "markdown", markdown.as_str()),
    ] {
        let path = directory.path().join(filename);
        std::fs::write(&path, content).unwrap();
        responses.insert(path.to_str().unwrap().to_string(), json!({"file_type": file_type,
            "content": content, "directory": directory.path(), "filename": filename, "size": content.len()}));
        text_cases.push(path);
    }
    let dispatcher = Arc::new(BuiltFixtureDispatcher {
        responses,
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let view = webkit6::WebView::builder()
        .web_context(context)
        .network_session(session)
        .build();
    bundle_viewers.borrow_mut().insert(view.as_ptr() as usize);
    let settings = webkit6::Settings::new();
    apply_widget_settings(&settings);
    settings.set_javascript_can_open_windows_automatically(false);
    view.set_settings(&settings);
    let process_stopped = Rc::new(Cell::new(false));
    let callback_stopped = process_stopped.clone();
    view.connect_web_process_terminated(move |_, reason| {
        eprintln!("built viewer process stopped: {reason:?}");
        callback_stopped.set(true);
    });
    quantum_ui::register_bridge(
        &view,
        dispatcher.clone(),
        runtime.handle().clone(),
        Arc::new(std::sync::Mutex::new(None)),
    );
    // A real viewport is required for layout/scroll assertions. This ordinary
    // window is created only after the enclosing test's private Xvfb guards.
    let window = gtk4::Window::builder()
        .title("Isolated native viewer harness")
        .default_width(800)
        .default_height(600)
        .child(&view)
        .build();
    window.present();

    for (path, content_type, width, height, required) in &image_cases {
        load_built_case(&view, path);
        wait_for_dom(&view, "ImageRenderer decode or visible error", "document.querySelector('.image-renderer img')?.naturalWidth > 0 || document.querySelector('.image-error')");
        let observation = evaluate(
            &view,
            r#"JSON.stringify((() => {
            const image = document.querySelector('.image-renderer img');
            const error = document.querySelector('.image-error');
            return {source: image.currentSrc, naturalWidth: image.naturalWidth, naturalHeight: image.naturalHeight,
                loaded: image.naturalWidth > 0, error: error?.innerText ?? null,
                errorVisible: Boolean(error && error.getBoundingClientRect().height > 0),
                fit: document.querySelector('.mode-fit')?.classList.contains('active') ?? false};
        })())"#,
        );
        eprintln!(
            "built viewer format={content_type} file={} observation={observation}",
            path.display()
        );
        assert!(observation["source"]
            .as_str()
            .unwrap()
            .starts_with("quantum://viewer-image/"));
        if observation["loaded"] == json!(false) {
            assert!(
                !required,
                "required image failed in the built ImageRenderer: {observation}"
            );
            assert_eq!(observation["errorVisible"], json!(true));
            assert!(observation["error"]
                .as_str()
                .unwrap()
                .contains("Failed to load image"));
            continue;
        }
        assert_eq!(observation["naturalWidth"], json!(width));
        assert_eq!(observation["naturalHeight"], json!(height));
        assert_eq!(observation["fit"], json!(true));
        for (selector, scale, label) in [
            (".mode-actual", 1.0, "100%"),
            ("button[aria-label='Zoom in']", 1.25, "125%"),
            ("button[aria-label='Zoom out']", 1.0, "100%"),
        ] {
            evaluate(
                &view,
                &format!(
                    "document.querySelector({}).click(); JSON.stringify(null)",
                    serde_json::to_string(selector).unwrap()
                ),
            );
            wait_for_dom(
                &view,
                "real image control changes zoom",
                &format!(
                    "document.querySelector('.zoom-label')?.textContent === {}",
                    serde_json::to_string(label).unwrap()
                ),
            );
            let dimensions = evaluate(&view, "JSON.stringify((() => { const image = document.querySelector('.image-renderer img'); return {width: parseFloat(image.style.width), height: parseFloat(image.style.height), renderedWidth: image.getBoundingClientRect().width, renderedHeight: image.getBoundingClientRect().height}; })())");
            assert_eq!(dimensions["width"].as_f64().unwrap(), *width as f64 * scale);
            assert_eq!(
                dimensions["height"].as_f64().unwrap(),
                *height as f64 * scale
            );
            assert!(dimensions["renderedWidth"].as_f64().unwrap() > 0.0);
            assert!(dimensions["renderedHeight"].as_f64().unwrap() > 0.0);
            assert!(
                (dimensions["renderedWidth"].as_f64().unwrap() - *width as f64 * scale).abs()
                    < 0.02
            );
            assert!(
                (dimensions["renderedHeight"].as_f64().unwrap() - *height as f64 * scale).abs()
                    < 0.02
            );
            eprintln!("built viewer control={selector} label={label} dimensions={dimensions}");
        }
        evaluate(
            &view,
            "document.querySelector('.mode-fit').click(); JSON.stringify(null)",
        );
        wait_for_dom(&view, "Fit restores unconstrained dimensions", "document.querySelector('.mode-fit')?.classList.contains('active') && document.querySelector('.image-renderer img')?.style.width === '' && document.querySelector('.image-renderer img')?.style.height === ''");
        assert_eq!(
            evaluate(&view, "JSON.stringify(window.nativeErrors)"),
            json!([])
        );
    }

    load_built_case(&view, &text_cases[0]);
    wait_for_dom(
        &view,
        "real virtual scroller layout",
        "document.querySelector('.virtual-scroller')?.clientHeight > 0",
    );
    assert_eq!(evaluate(&view, "JSON.stringify(document.querySelector('.text-content').textContent.includes('line 900: needle'))"), json!(false));
    search_built_viewer(&view, "needle", "1 of 2");
    press_viewer_key(&view, "Enter", false, false);
    wait_for_dom(&view, "offscreen match appears", "document.querySelector('.match-indicator')?.textContent === '2 of 2' && document.querySelector('.search-match-current')?.closest('[data-line]')?.dataset.line === '901'");
    let state = current_search_state(&view, ".virtual-scroller");
    eprintln!("built viewer thousand-line search={state}");
    if state["visible"] != json!(true) {
        findings.push(format!(
            "thousand-line current search match is outside the viewport: {state}"
        ));
    }
    assert!(state["scrollTop"].as_f64().unwrap() > 10000.0);
    assert!(
        evaluate(
            &view,
            "JSON.stringify(document.querySelectorAll('.text-line').length)"
        )
        .as_u64()
        .unwrap()
            < 200
    );
    press_viewer_key(&view, "Enter", false, false);
    wait_for_dom(
        &view,
        "search wraps to first result",
        "document.querySelector('.match-indicator')?.textContent === '1 of 2'",
    );
    press_viewer_key(&view, "Enter", false, true);
    wait_for_dom(
        &view,
        "Shift+Enter wraps to last result",
        "document.querySelector('.match-indicator')?.textContent === '2 of 2'",
    );
    press_viewer_key(&view, "Escape", false, false);
    wait_for_dom(
        &view,
        "Escape closes search only",
        "!document.querySelector('.search-bar')",
    );
    assert!(!dispatcher
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|(method, _)| method == "view.hide"));
    press_viewer_key(&view, "Escape", false, false);
    pump_until("second Escape asks host to close this viewer", || {
        dispatcher
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(method, params)| {
                method == "view.hide"
                    && params["name"] == json!("plugin/file-viewer/file-viewer#native-bundle")
            })
    });
    assert_eq!(
        evaluate(&view, "JSON.stringify(window.nativeErrors)"),
        json!([])
    );

    load_built_case(&view, &text_cases[1]);
    wait_for_dom(
        &view,
        "real JSON fold gutter",
        "document.querySelectorAll('.fold-marker').length >= 4",
    );
    for line in [3, 2, 7] {
        evaluate(&view, &format!("Array.from(document.querySelectorAll('.gutter-line')).find(line => line.querySelector('.line-number').textContent === '{line}').querySelector('button').click(); JSON.stringify(null)"));
        wait_for_dom(&view, "JSON fold collapsed", &format!("Array.from(document.querySelectorAll('.gutter-line')).find(line => line.querySelector('.line-number').textContent === '{line}')?.querySelector('button')?.classList.contains('collapsed')"));
    }
    assert_eq!(
        evaluate(
            &view,
            "JSON.stringify(Boolean(document.querySelector('.json-line[data-line=\"4\"]')))"
        ),
        json!(false)
    );
    search_built_viewer(&view, "needle", "1 of 2");
    wait_for_dom(
        &view,
        "search opens both enclosing JSON folds",
        "document.querySelector('.json-line[data-line=\"4\"] .search-match-current')",
    );
    assert_eq!(evaluate(&view, "JSON.stringify(Array.from(document.querySelectorAll('.gutter-line')).find(line => line.querySelector('.line-number').textContent === '7').querySelector('button').classList.contains('collapsed'))"), json!(true));
    let state = current_search_state(&view, ".json-fold-renderer");
    eprintln!("built viewer JSON folded search={state}");
    if state["visible"] != json!(true) {
        findings.push(format!(
            "JSON folded search match is not fully visible: {state}"
        ));
    }
    press_viewer_key(&view, "Escape", false, false);
    wait_for_dom(&view, "JSON search closes without recollapsing match", "!document.querySelector('.search-bar') && document.querySelector('.json-line[data-line=\"4\"]') && !document.querySelector('mark.search-match')");
    assert_eq!(
        evaluate(&view, "JSON.stringify(window.nativeErrors)"),
        json!([])
    );

    load_built_case(&view, &text_cases[2]);
    wait_for_dom(
        &view,
        "real Markdown renderer",
        "document.querySelector('.markdown-renderer strong')?.textContent === 'needle'",
    );
    search_built_viewer(&view, "needle", "1 of 2");
    press_viewer_key(&view, "Enter", false, false);
    wait_for_dom(&view, "Markdown selects lower rendered match", "document.querySelector('.match-indicator')?.textContent === '2 of 2' && document.querySelector('.search-match-current')?.parentElement?.tagName === 'EM'");
    let state = current_search_state(&view, ".markdown-content");
    eprintln!("built viewer Markdown search={state}");
    if state["visible"] != json!(true) {
        findings.push(format!(
            "Markdown current search match is outside the viewport: {state}"
        ));
    }
    assert!(state["scrollTop"].as_f64().unwrap() > 1000.0);
    wait_for_dom(
        &view,
        "Mermaid finished or shows explicit error",
        "document.querySelector('.diagram-block svg') || document.querySelector('.diagram-error')",
    );
    let diagram = evaluate(&view, "JSON.stringify({rendered: Boolean(document.querySelector('.diagram-block svg')), error: document.querySelector('.diagram-error')?.innerText ?? null, matches: document.querySelectorAll('.diagram-block mark').length})");
    eprintln!("built viewer Mermaid={diagram}");
    assert_eq!(diagram["matches"], json!(0));
    search_built_viewer(&view, "**needle**", "No matches");
    assert_eq!(
        evaluate(&view, "JSON.stringify(window.nativeErrors)"),
        json!([])
    );
    assert!(
        !process_stopped.get(),
        "built viewer renderer terminated unexpectedly"
    );
    quantum_ui::viewer_image_resources::suspend_view(&view, true);
    view.terminate_web_process();
    pump_until("built viewer renderer terminated", || process_stopped.get());
    window.destroy();
    bundle_viewers
        .borrow_mut()
        .remove(&(view.as_ptr() as usize));
    assert!(
        findings.is_empty(),
        "built viewer behavioral findings: {}",
        findings.join("\n")
    );
    eprintln!("built viewer integrated assertions passed: actual App, client, ImageRenderer controls, virtualized text, JSON folds, rendered Markdown, and host bridge/grants");
}
