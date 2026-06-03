//! Discovered plugin descriptions and recipe types. The walker (in
//! `walker.rs`) produces these; the daemon's startup wiring consumes them.

use std::path::PathBuf;
use std::time::Duration;

/// A plugin discovered under `~/.config/quantum/plugins/<name>/`. The
/// walker emits one of these per subdirectory.
#[derive(Debug, Clone)]
pub struct PluginDescription {
    pub name: String,
    pub dir: PathBuf,
    pub polled_scripts: Vec<PolledScript>,
    pub idle_scripts: Vec<IdleScript>,
    pub actions: Vec<ActionScript>,
    pub views: Vec<ViewBundle>,
}

/// A script under `scripts/` that has been opted into polling by the
/// plugin's `config.toml`.
#[derive(Debug, Clone)]
pub struct PolledScript {
    pub command: PathBuf,
    pub interval: Duration,
    pub channel: String,
}

/// A script under `scripts/` that is NOT polled (no `config.toml` entry).
/// Idle scripts are still invokable on demand but never auto-fire.
#[derive(Debug, Clone)]
pub struct IdleScript {
    pub command: PathBuf,
    pub channel: String,
}

/// A script under `actions/`. Always invokable via the daemon's
/// `action.invoke` IPC method; never scheduled.
#[derive(Debug, Clone)]
pub struct ActionScript {
    pub name: String,
    pub command: PathBuf,
}

/// A view bundle under `views/<name>/`. Served at
/// `quantum://plugin/<plugin>/views/<name>/<file>` by the scheme handler.
#[derive(Debug, Clone)]
pub struct ViewBundle {
    pub name: String,
    pub dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_constructs_with_all_recipe_kinds() {
        let d = PluginDescription {
            name: "moon".into(),
            dir: PathBuf::from("/tmp/moon"),
            polled_scripts: vec![PolledScript {
                command: PathBuf::from("/tmp/moon/scripts/moon.sh"),
                interval: Duration::from_secs(3600),
                channel: "moon.moon".into(),
            }],
            idle_scripts: vec![IdleScript {
                command: PathBuf::from("/tmp/moon/scripts/idle.sh"),
                channel: "moon.idle".into(),
            }],
            actions: vec![ActionScript {
                name: "open".into(),
                command: PathBuf::from("/tmp/moon/actions/open.sh"),
            }],
            views: vec![ViewBundle {
                name: "widget".into(),
                dir: PathBuf::from("/tmp/moon/views/widget"),
            }],
        };
        assert_eq!(d.name, "moon");
        assert_eq!(d.polled_scripts.len(), 1);
        assert_eq!(d.idle_scripts.len(), 1);
        assert_eq!(d.actions.len(), 1);
        assert_eq!(d.views.len(), 1);
        let _ = format!("{d:?}");
        let _ = d.clone();
    }
}
