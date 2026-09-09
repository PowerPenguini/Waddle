use super::*;
use iced::advanced::{
    Layout, layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
};
use iced::{Color, Point, Rectangle, Size, Theme, mouse};

// Uses the real widget layouts and GPU rasterizer: isolated SVG tests miss
// button constraints, while shape symmetry alone misses equally thinned arms.
#[test]
#[ignore = "requires a headless wgpu adapter"]
fn chevrons_keep_even_edges_when_positioned_at_fractional_scales() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut renderer = <iced::Renderer as Headless>::new(
            iced::Font::DEFAULT, iced::Pixels(14.0), Some("wgpu"),
        ).await.expect("headless wgpu adapter");
        let temp = tempfile::tempdir().unwrap();
        let (mut app, _) = App::new();
        app.view_preferences = crate::app::view_preferences::Preferences::empty_at(temp.path().join("preferences"));
        let _ = app.update(Message::SortBy(fs::SortKey::Name));
        for name in ["sort-up", "sort-down", "back", "forward", "parent"] {
            if name == "sort-down" { let _ = app.update(Message::SortBy(fs::SortKey::Name)); }
            for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
                let mut brightnesses = Vec::new();
                for offset in [8.0, 8.2, 8.4, 8.6, 8.8, 9.0] {
                    let bounds = Rectangle::with_size(Size::new(64.0, 64.0));
                    renderer.reset(bounds);
                    let mut element = match name {
                        "sort-up" | "sort-down" => View::new(&app).sort_header("", fs::SortKey::Name, 48),
                        _ => toolbar_button(match name {
                            "back" => include_bytes!("../../ui/icons/back.svg"),
                            "forward" => include_bytes!("../../ui/icons/forward.svg"),
                            _ => include_bytes!("../../ui/icons/up.svg"),
                        }, name, true, Message::Noop, Color::WHITE, Color::BLACK).into(),
                    };
                    let mut tree = Tree::new(element.as_widget());
                    let node = element.as_widget_mut().layout(&mut tree, &renderer,
                        &layout::Limits::new(Size::ZERO, bounds.size())).move_to(Point::new(offset, offset));
                    element.as_widget().draw(&tree, &mut renderer, &Theme::Dark,
                        &iced::advanced::renderer::Style { text_color: Color::WHITE },
                        Layout::new(&node), mouse::Cursor::Unavailable, &bounds);
                    let width = (64.0 * scale) as u32;
                    let pixels = renderer.screenshot(Size::new(width, width), scale, Color::BLACK);
                    if let Some(directory) = std::env::var_os("WADDLE_RENDER_ARTIFACT_DIR") {
                        std::fs::create_dir_all(&directory).unwrap();
                        image::save_buffer(std::path::PathBuf::from(directory).join(format!("{name}-{scale}-{offset}.png")),
                            &pixels, width, width, image::ColorType::Rgba8).unwrap();
                    }
                    let brightness: u64 = pixels.as_chunks::<4>().0.iter().map(|p| u64::from(p[0])).sum();
                    assert!(brightness > 0, "{name} must be visible");
                    brightnesses.push(brightness);
                }
                let min = *brightnesses.iter().min().unwrap() as f64;
                let max = *brightnesses.iter().max().unwrap() as f64;
                // Translation may change edge coverage slightly, but must not
                // drop whole rows/columns and visibly thin the chevron.
                let variation = (max - min) / max;
                assert!(variation < 0.05,
                    "{name} at {scale}x changes stroke brightness by {:.1}% when moved: {brightnesses:?}", variation * 100.0);
            }
        }
    });
}
