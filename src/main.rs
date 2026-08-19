mod app;
mod fs;
mod theme;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| {
            attributes.with_transparent(true).with_blur(true)
        })
        .select()?;

    app::run()
}
