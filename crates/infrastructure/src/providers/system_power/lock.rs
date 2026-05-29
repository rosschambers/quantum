use std::path::PathBuf;

pub(crate) fn resolve_lock_command(
    config: Option<&str>,
    which: impl Fn(&str) -> Option<PathBuf>,
) -> Option<Vec<String>> {
    if let Some(s) = config {
        if s.is_empty() {
            return None;
        }
        return shell_words::split(s).ok();
    }
    for name in ["hyprlock", "swaylock", "gtklock"] {
        if which(name).is_some() {
            return Some(vec![name.into()]);
        }
    }
    if which("loginctl").is_some() {
        return Some(vec!["loginctl".into(), "lock-session".into()]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_override_wins_over_probe() {
        let r = resolve_lock_command(Some("hyprlock --grace 0"), |_| {
            Some(PathBuf::from("/usr/bin/swaylock"))
        });
        assert_eq!(
            r,
            Some(vec!["hyprlock".into(), "--grace".into(), "0".into()])
        );
    }

    #[test]
    fn empty_override_returns_none() {
        let r = resolve_lock_command(Some(""), |_| Some(PathBuf::from("/usr/bin/swaylock")));
        assert_eq!(r, None);
    }

    #[test]
    fn probes_hyprlock_first() {
        let r = resolve_lock_command(None, |name| match name {
            "hyprlock" | "swaylock" | "loginctl" => Some(PathBuf::from(format!("/usr/bin/{name}"))),
            _ => None,
        });
        assert_eq!(r, Some(vec!["hyprlock".into()]));
    }

    #[test]
    fn probes_swaylock_when_hyprlock_missing() {
        let r = resolve_lock_command(None, |name| match name {
            "swaylock" | "loginctl" => Some(PathBuf::from(format!("/usr/bin/{name}"))),
            _ => None,
        });
        assert_eq!(r, Some(vec!["swaylock".into()]));
    }

    #[test]
    fn probes_gtklock_when_first_two_missing() {
        let r = resolve_lock_command(None, |name| match name {
            "gtklock" | "loginctl" => Some(PathBuf::from(format!("/usr/bin/{name}"))),
            _ => None,
        });
        assert_eq!(r, Some(vec!["gtklock".into()]));
    }

    #[test]
    fn falls_back_to_loginctl_when_no_lockers() {
        let r = resolve_lock_command(None, |name| {
            (name == "loginctl").then(|| PathBuf::from("/usr/bin/loginctl"))
        });
        assert_eq!(r, Some(vec!["loginctl".into(), "lock-session".into()]));
    }

    #[test]
    fn nothing_available_returns_none() {
        let r = resolve_lock_command(None, |_| None);
        assert_eq!(r, None);
    }
}
