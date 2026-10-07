//! Exact image capabilities owned by one live WebView, never filesystem routes.

use gio::prelude::*;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};
use std::rc::Rc;
use std::sync::Arc;
use webkit6::prelude::*;

const PREFIX: &str = "quantum://viewer-image/";

pub(crate) struct PreparedImage {
    file: File,
    mime_type: String,
    token: String,
}

impl PreparedImage {
    fn read_bytes(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = self.file.read_at(&mut buffer, bytes.len() as u64)?;
            if count == 0 {
                return Ok(bytes);
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
}

/// Run only on a blocking worker, after a successful dispatcher response.
pub(crate) fn prepare_image(value: &Value) -> io::Result<Option<PreparedImage>> {
    if value.get("file_type").and_then(Value::as_str) != Some("image") {
        return Ok(None);
    }
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid viewer image descriptor",
        )
    };
    let directory = value
        .get("directory")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let filename = value
        .get("filename")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let uri = value
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let mime_type = value
        .get("mime_type")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let mut components = Path::new(filename).components();
    if !Path::new(directory).is_absolute()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || !mime_type.starts_with("image/")
        || !mime_type
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || b"/.-+".contains(&character))
    {
        return Err(invalid());
    }
    let path = Path::new(directory).join(filename);
    // The shipped backend emits a RAW path, including literal '%' and '#'.
    // Never URI-decode that string. Build an escaped Gio URI from its agreeing
    // canonical descriptor fields, then let Gio decode the escaped URI.
    if uri != format!("file://{}", path.display()) {
        return Err(invalid());
    }
    let escaped_uri = gio::File::for_path(&path).uri();
    let decoded_path = gio::File::for_uri(&escaped_uri)
        .path()
        .ok_or_else(invalid)?;
    let file = open_canonical_file(&decoded_path)?;
    let mut entropy = [0; 32];
    File::open("/dev/urandom")?.read_exact(&mut entropy)?;
    let token = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Some(PreparedImage {
        file,
        mime_type: mime_type.to_string(),
        token,
    }))
}

fn open_canonical_file(path: &Path) -> io::Result<File> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "image path is not a canonical regular file",
        )
    };
    let mut components = path.components().peekable();
    if components.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    let directory_flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(directory_flags)
        .open("/")?;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        let name = CString::new(name.as_bytes()).map_err(|_| invalid())?;
        let is_leaf = components.peek().is_none();
        let flags = if is_leaf {
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC
        } else {
            directory_flags
        };
        // Pin each directory before looking up the next component. A renamed
        // directory cannot redirect the remaining traversal through a symlink.
        // SAFETY: both the directory descriptor and NUL-terminated name are live.
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new, uniquely owned descriptor.
        let opened = unsafe { File::from_raw_fd(descriptor) };
        if is_leaf {
            if !opened.metadata()?.is_file() {
                return Err(invalid());
            }
            return Ok(opened);
        }
        directory = opened;
    }
    Err(invalid())
}

struct GrantState {
    generation: Arc<()>,
    document_generation: Arc<()>,
    active: bool,
    navigating: bool,
    disposed: bool,
    images: HashMap<String, Arc<PreparedImage>>,
}

impl Default for GrantState {
    fn default() -> Self {
        Self {
            generation: Arc::new(()),
            document_generation: Arc::new(()),
            active: true,
            navigating: false,
            disposed: false,
            images: HashMap::new(),
        }
    }
}

impl GrantState {
    fn invalidate(&mut self) {
        self.generation = Arc::new(());
        self.images.clear();
    }

    fn begin_read(&mut self) -> Option<Arc<()>> {
        if !self.active || self.navigating || self.disposed {
            return None;
        }
        self.invalidate();
        Some(self.generation.clone())
    }

    fn is_current(&self, generation: &Arc<()>) -> bool {
        self.active
            && !self.navigating
            && !self.disposed
            && Arc::ptr_eq(&self.generation, generation)
    }

    fn is_document_current(&self, generation: &Arc<()>) -> bool {
        // Warm hide revokes image access, not the retained page's callbacks.
        !self.navigating && !self.disposed && Arc::ptr_eq(&self.document_generation, generation)
    }

    fn issue(&mut self, generation: &Arc<()>, image: PreparedImage) -> Option<String> {
        if !self.is_current(generation) || self.images.contains_key(&image.token) {
            return None;
        }
        let token = image.token.clone();
        self.images.insert(token.clone(), Arc::new(image));
        Some(token)
    }

    fn resolve(&self, token: &str) -> Option<(Arc<()>, Arc<PreparedImage>)> {
        if !self.active || self.navigating || self.disposed {
            return None;
        }
        self.images
            .get(token)
            .map(|image| (self.generation.clone(), image.clone()))
    }

    fn suspend(&mut self, disposed: bool) {
        self.invalidate();
        if disposed {
            self.document_generation = Arc::new(());
        }
        self.active = false;
        self.disposed |= disposed;
    }

    fn activate(&mut self) {
        if !self.disposed {
            self.active = true;
        }
    }

    fn navigation(&mut self, started: bool) {
        if started {
            self.invalidate();
            self.document_generation = Arc::new(());
        }
        self.navigating = started;
    }
}

pub(crate) struct ViewResources {
    owner: glib::WeakRef<webkit6::WebView>,
    state: RefCell<GrantState>,
}

impl ViewResources {
    pub(crate) fn document_generation(&self) -> Arc<()> {
        self.state.borrow().document_generation.clone()
    }

    pub(crate) fn is_document_current(&self, generation: &Arc<()>) -> bool {
        self.owner.upgrade().is_some() && self.state.borrow().is_document_current(generation)
    }

    pub(crate) fn begin_read(&self) -> Option<Arc<()>> {
        self.state.borrow_mut().begin_read()
    }

    pub(crate) fn is_current(&self, generation: &Arc<()>) -> bool {
        self.owner.upgrade().is_some() && self.state.borrow().is_current(generation)
    }

    pub(crate) fn issue(&self, generation: &Arc<()>, image: PreparedImage) -> Option<String> {
        self.owner.upgrade()?;
        self.state
            .borrow_mut()
            .issue(generation, image)
            .map(|token| format!("{PREFIX}{token}"))
    }
}

thread_local! {
    static VIEWS: RefCell<HashMap<usize, Rc<ViewResources>>> = RefCell::new(HashMap::new());
}

pub(crate) fn bind_view(view: &webkit6::WebView) -> Rc<ViewResources> {
    let identity = view.as_ptr() as usize;
    if let Some(resources) = resources_for(view) {
        return resources;
    }
    let resources = Rc::new(ViewResources {
        owner: view.downgrade(),
        state: RefCell::new(GrantState::default()),
    });
    VIEWS.with(|views| views.borrow_mut().insert(identity, resources.clone()));
    let navigation_resources = resources.clone();
    view.connect_load_changed(move |_, event| match event {
        webkit6::LoadEvent::Started => navigation_resources.state.borrow_mut().navigation(true),
        webkit6::LoadEvent::Committed => navigation_resources.state.borrow_mut().navigation(false),
        _ => {}
    });
    let terminated_resources = resources.clone();
    view.connect_web_process_terminated(move |_, _| {
        terminated_resources.state.borrow_mut().navigation(true)
    });
    view.add_weak_ref_notify_local(move || {
        let _ = VIEWS.try_with(|views| {
            if let Some(resources) = views.borrow_mut().remove(&identity) {
                resources.state.borrow_mut().suspend(true);
            }
        });
    });
    resources
}

fn resources_for(view: &webkit6::WebView) -> Option<Rc<ViewResources>> {
    VIEWS.with(|views| {
        views
            .borrow()
            .get(&(view.as_ptr() as usize))
            .filter(|resources| resources.owner.upgrade().as_ref() == Some(view))
            .cloned()
    })
}

/// Revoke grants and pending reads before hiding or permanently destroying a view.
pub fn suspend_view(view: &webkit6::WebView, disposed: bool) {
    if let Some(resources) = resources_for(view) {
        resources.state.borrow_mut().suspend(disposed);
    }
}

/// Resume a warm view; disposed views can never be reactivated.
pub fn activate_view(view: &webkit6::WebView) {
    if let Some(resources) = resources_for(view) {
        resources.state.borrow_mut().activate();
    }
}

pub(crate) fn invalidate_view(view: &webkit6::WebView) {
    if let Some(resources) = resources_for(view) {
        resources.state.borrow_mut().invalidate();
    }
}

pub(crate) fn bind_window(window: &gtk4::Window, view: &webkit6::WebView) {
    use gtk4::prelude::*;
    let weak_view = view.downgrade();
    window.connect_close_request(move |_| {
        if let Some(view) = weak_view.upgrade() {
            suspend_view(&view, true);
        }
        glib::Propagation::Proceed
    });
}

fn not_found(request: &webkit6::URISchemeRequest) {
    request.finish_error(&mut glib::Error::new(glib::FileError::Noent, "not found"));
}

/// Handle the capability namespace, rejecting absent provenance uniformly.
/// All file reads run on Gio's blocking pool; finish happens on the GLib thread.
pub fn serve_request(request: &webkit6::URISchemeRequest) -> bool {
    let Some(uri) = request.uri() else {
        return false;
    };
    let Some(token) = uri.strip_prefix(PREFIX) else {
        return false;
    };
    let authorization = request
        .web_view()
        .and_then(|view| resources_for(&view))
        .and_then(|resources| {
            let resolved = resources.state.borrow().resolve(token);
            resolved.map(|(generation, image)| (resources, generation, image))
        });
    let Some((resources, generation, image)) = authorization else {
        not_found(request);
        return true;
    };
    let request = request.clone();
    glib::MainContext::default().spawn_local(async move {
        let worker_image = image.clone();
        let result = gio::spawn_blocking(move || worker_image.read_bytes()).await;
        if !resources.is_current(&generation) {
            not_found(&request);
            return;
        }
        match result {
            Ok(Ok(bytes)) => {
                let length = bytes.len() as i64;
                let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                let response = webkit6::URISchemeResponse::new(&stream, length);
                let headers =
                    webkit6::soup::MessageHeaders::new(webkit6::soup::MessageHeadersType::Response);
                headers.append("Cache-Control", "no-store");
                headers.append("X-Content-Type-Options", "nosniff");
                response.set_http_headers(headers);
                response.set_content_type(&image.mime_type);
                response.set_status(200, None);
                request.finish_with_response(&response);
            }
            _ => not_found(&request),
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(path: &std::path::Path) -> Value {
        json!({"file_type": "image", "directory": path.parent().unwrap(),
            "filename": path.file_name().unwrap().to_str().unwrap(),
            "uri": format!("file://{}", path.display()), "mime_type": "image/svg+xml"})
    }

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, PreparedImage) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.svg");
        std::fs::write(&path, b"original bytes").unwrap();
        let image = prepare_image(&response(&path)).unwrap().unwrap();
        (directory, path, image)
    }

    #[test]
    fn unissued_foreign_and_extension_only_requests_are_rejected() {
        let (_directory, _path, image) = fixture();
        let mut owner = GrantState::default();
        let foreign = GrantState::default();
        assert!(owner.resolve(&"0".repeat(64)).is_none());
        assert!(owner.resolve("image.svg").is_none());
        let generation = owner.begin_read().unwrap();
        let token = owner.issue(&generation, image).unwrap();
        assert!(owner.resolve(&token).is_some());
        assert!(foreign.resolve(&token).is_none());
    }

    #[test]
    fn hide_navigation_new_read_and_disposal_revoke_pending_and_issued_grants() {
        for disposal in [false, true] {
            let (_directory, _path, image) = fixture();
            let mut state = GrantState::default();
            let generation = state.begin_read().unwrap();
            let token = state.issue(&generation, image).unwrap();
            state.suspend(disposal);
            assert!(
                !state.is_current(&generation),
                "in-flight resource reads must also be rejected"
            );
            assert!(state.resolve(&token).is_none());
            assert!(state.begin_read().is_none());
            state.activate();
            let (_directory, _path, image) = fixture();
            assert!(state.issue(&generation, image).is_none());
            assert_eq!(state.begin_read().is_none(), disposal);
        }
        let (_directory, _path, image) = fixture();
        let mut state = GrantState::default();
        let old = state.begin_read().unwrap();
        let latest = state.begin_read().unwrap();
        assert!(!state.is_current(&old));
        assert!(state.is_current(&latest));
        assert!(state.issue(&old, image).is_none());
    }

    #[test]
    fn original_descriptor_survives_symlink_replacement_and_concurrent_reads() {
        let (directory, path, image) = fixture();
        let replacement = directory.path().join("replacement.svg");
        std::fs::write(&replacement, b"secret replacement").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&replacement, &path).unwrap();
        let image = Arc::new(image);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let image = image.clone();
                scope.spawn(move || {
                    for _ in 0..3 {
                        assert_eq!(image.read_bytes().unwrap(), b"original bytes");
                    }
                });
            }
        });
    }

    #[test]
    fn raw_uri_literal_percent_hash_spaces_and_unicode_are_not_decoded_as_uri_syntax() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("space # %2F \u{00e9}.svg");
        std::fs::write(&path, b"literal filename").unwrap();
        let image = prepare_image(&response(&path)).unwrap().unwrap();
        assert_eq!(image.read_bytes().unwrap(), b"literal filename");
        assert_eq!(image.token.len(), 64);
        let second = prepare_image(&response(&path)).unwrap().unwrap();
        assert_ne!(image.token, second.token);
    }

    #[test]
    fn only_successful_image_descriptors_with_matching_raw_uri_are_prepared() {
        let (_directory, path, _image) = fixture();
        let mut value = response(&path);
        value["file_type"] = json!("video");
        assert!(prepare_image(&value).unwrap().is_none());
        value["file_type"] = json!("image");
        value["uri"] = json!("file:///unrelated.svg");
        assert!(prepare_image(&value).is_err());
        value = response(&path);
        value["mime_type"] = json!("text/html");
        assert!(prepare_image(&value).is_err());
        value = response(&path);
        value["filename"] = json!("../image.svg");
        assert!(prepare_image(&value).is_err());
    }

    #[test]
    fn navigation_commit_cannot_reactivate_hidden_or_disposed_owner() {
        for disposed in [false, true] {
            let mut state = GrantState::default();
            let generation = state.begin_read().unwrap();
            state.navigation(true);
            assert!(!state.is_current(&generation));
            assert!(state.begin_read().is_none());
            state.suspend(disposed);
            state.navigation(false);
            assert!(state.begin_read().is_none());
            assert!(!state.is_current(&generation));
        }
    }

    #[test]
    fn warm_hide_preserves_document_delivery_but_navigation_and_disposal_do_not() {
        let mut state = GrantState::default();
        let document = state.document_generation.clone();
        let older_read = state.begin_read().unwrap();
        let latest_read = state.begin_read().unwrap();
        assert!(state.is_document_current(&document));
        assert!(!state.is_current(&older_read));
        assert!(state.is_current(&latest_read));
        state.navigation(true);
        state.navigation(false);
        assert!(!state.is_document_current(&document));
        let document = state.document_generation.clone();
        assert!(state.is_document_current(&document));
        state.suspend(false);
        assert!(
            state.is_document_current(&document),
            "warm hidden document still owns its pending callbacks"
        );
        assert!(
            state.begin_read().is_none(),
            "hidden document cannot issue image grants"
        );
        state.activate();
        assert!(state.is_document_current(&document));
        state.suspend(true);
        state.activate();
        assert!(!state.is_document_current(&document));
    }

    #[test]
    fn prepare_rejects_leaf_symlink_replacement_before_open() {
        let (directory, path, _image) = fixture();
        let descriptor = response(&path);
        let secret = directory.path().join("secret.svg");
        std::fs::write(&secret, b"unapproved secret bytes").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&secret, &path).unwrap();
        assert!(
            prepare_image(&descriptor).is_err(),
            "must not follow a replacement leaf symlink"
        );
    }

    #[test]
    fn prepare_rejects_parent_directory_symlink_replacement_before_open() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("original");
        let other = directory.path().join("other");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(&other).unwrap();
        let path = parent.join("image.svg");
        std::fs::write(&path, b"approved bytes").unwrap();
        std::fs::write(other.join("image.svg"), b"unapproved bytes").unwrap();
        let descriptor = response(&path);
        std::fs::rename(&parent, directory.path().join("moved")).unwrap();
        std::os::unix::fs::symlink(&other, &parent).unwrap();
        assert!(
            prepare_image(&descriptor).is_err(),
            "must not follow a replacement ancestor symlink"
        );
    }

    #[test]
    fn prepare_rejects_fifo_replacement_without_waiting_for_a_writer() {
        let (_directory, path, _image) = fixture();
        let descriptor = response(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker =
            std::thread::spawn(move || sender.send(prepare_image(&descriptor).is_err()).unwrap());
        let result = receiver.recv_timeout(std::time::Duration::from_millis(500));
        // Unblock the old implementation before asserting, so a red run never
        // leaves a hung worker or relies on the outer test-process timeout.
        if result.is_err() {
            let _writer = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .unwrap();
        }
        worker.join().unwrap();
        assert_eq!(
            result.ok(),
            Some(true),
            "a FIFO must fail promptly without a writer"
        );
    }
}
