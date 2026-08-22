pub use quantum_domain::WindowMode;

#[derive(Debug, Clone)]
pub enum WindowRequest {
    Open {
        view: String,
        mode: WindowMode,
        args: Option<serde_json::Value>,
    },
    SetHeight {
        view: String,
        height: u32,
    },
    /// Set the pointer input region of an already-open window. The bar uses
    /// this to clip its full-height surface's input to the visible strip
    /// (plus any open menu); `None` resets to the strip-only default.
    SetInputRegion {
        view: String,
        region: Option<quantum_domain::WindowInputRegion>,
    },
    /// Tear down a window entirely. Used by the bar multiplexer when a
    /// monitor disconnects so its bar window is released along with
    /// the underlying layer-shell surface, instead of merely hidden.
    Close {
        view: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_request_debug_includes_view_name() {
        let req = WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Toggle,
            args: None,
        };
        let dbg = format!("{:?}", req);
        assert!(dbg.contains("launcher"));
        assert!(dbg.contains("Toggle"));
    }
}
