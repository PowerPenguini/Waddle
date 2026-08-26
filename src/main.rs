mod app;
mod fs;
mod journal;
mod theme;
mod transfer;
#[path = "clipboard.rs"]
mod transfer_formats;

fn main() -> iced::Result {
    prefer_transparent_wayland_backend();
    app::run()
}

#[cfg(target_os = "linux")]
fn prefer_transparent_wayland_backend() {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("WGPU_BACKEND").is_none() {
        // SAFETY: this runs before Iced starts its executor or creates any
        // renderer threads, so no concurrent environment access is possible.
        unsafe { std::env::set_var("WGPU_BACKEND", "gl") };
    }
}

#[cfg(not(target_os = "linux"))]
fn prefer_transparent_wayland_backend() {}
