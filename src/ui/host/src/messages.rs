pub use quantum_domain::WindowMode;

#[derive(Debug, Clone)]
pub enum WindowRequest {
    Open {
        view: String,
        mode: WindowMode,
    },
    SetHeight {
        view: String,
        height: u32,
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
        };
        let dbg = format!("{:?}", req);
        assert!(dbg.contains("launcher"));
        assert!(dbg.contains("Toggle"));
    }
}
