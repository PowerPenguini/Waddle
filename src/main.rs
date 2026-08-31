mod app;
mod file_manager_service;
mod fs;
mod journal;
mod launch;
mod theme;
mod transfer;
#[path = "clipboard.rs"]
mod transfer_formats;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if file_manager_service::requested() {
        return file_manager_service::run();
    }
    if launch::detach_terminal_invocation()? {
        return Ok(());
    }
    prefer_transparent_wayland_backend();
    app::run()?;
    Ok(())
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
