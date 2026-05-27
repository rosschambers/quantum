pub mod declarative_shell;
pub mod desktop_apps;
pub mod hyprland_windows;
pub mod shell_command;

pub use declarative_shell::DeclarativeShellProvider;
pub use desktop_apps::DesktopAppsProvider;
pub use hyprland_windows::HyprlandWindowsProvider;
pub use shell_command::ShellCommandProvider;
