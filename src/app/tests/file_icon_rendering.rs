use super::*;
use iced::advanced::{
    Layout, layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
};
use iced::{Color, Point, Rectangle, Size, Theme, mouse};

const KINDS: [EntryIconKind; 11] = [
    EntryIconKind::Folder,
    EntryIconKind::Generic,
    EntryIconKind::Document,
    EntryIconKind::Code,
    EntryIconKind::Pdf,
    EntryIconKind::Image,
    EntryIconKind::Audio,
    EntryIconKind::Video,
    EntryIconKind::Archive,
    EntryIconKind::Spreadsheet,
    EntryIconKind::Presentation,
];

#[test]
fn bundled_file_icons_and_drag_previews_render_without_fonts() {
    use resvg::{tiny_skia, usvg};

    let options = usvg::Options::default();
    for kind in KINDS {
        let data = entry_icon_asset(kind);
        assert!(!std::str::from_utf8(data).unwrap().contains("<text"));
        let tree = usvg::Tree::from_data(data, &options).unwrap();
        for size in [16, 24, 48, 96, 128] {
            let mut pixels = tiny_skia::Pixmap::new(size, size).unwrap();
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(size as f32 / 24.0, size as f32 / 24.0),
                &mut pixels.as_mut(),
            );
            let solid_colors: std::collections::HashSet<_> = pixels
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] == 255)
                .map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect();
            assert!(
                solid_colors.len() >= if kind == EntryIconKind::Folder { 1 } else { 2 },
                "{kind:?} at {size}px lost its contrasting mark"
            );
        }
        let preview = native_dnd::preview_svg(crate::transfer::Preview {
            icon: data,
            count: 1,
            copy: false,
            background: [30, 30, 30, 255],
            icon_color: [255, 0, 255, 255],
            accent: [30, 130, 220, 255],
            badge_text: [255; 4],
        })
        .unwrap();
        let tree = usvg::Tree::from_data(&preview, &options).unwrap();
        let mut pixels = tiny_skia::Pixmap::new(64, 64).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::identity(),
            &mut pixels.as_mut(),
        );
        // Only folders follow the theme tint; file marks keep their artwork colors.
        assert_eq!(
            pixels
                .data()
                .as_chunks::<4>()
                .0
                .contains(&[255, 0, 255, 255]),
            kind == EntryIconKind::Folder,
            "incorrect drag-preview tint for {kind:?}"
        );
    }
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn file_icon_widgets_preserve_colors_at_each_scale() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut renderer = <iced::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("wgpu"),
        )
        .await
        .expect("headless wgpu adapter");
        for (name, background) in [
            ("dark", Color::from_rgb8(30, 30, 30)),
            ("light", Color::from_rgb8(248, 249, 251)),
            ("selected", Color::from_rgb8(20, 73, 108)),
        ] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let accent_rgb = if name == "light" {
                    [24, 139, 118]
                } else {
                    [203, 93, 157]
                };
                let accent = Color::from_rgb8(accent_rgb[0], accent_rgb[1], accent_rgb[2]);
                let bounds = Rectangle::with_size(Size::new(1056.0, 248.0));
                renderer.reset(bounds);
                for (row, size) in [24.0, 48.0, 80.0].into_iter().enumerate() {
                    for (column, kind) in KINDS.into_iter().enumerate() {
                        let mut element: iced::Element<'_, Message> =
                            entry_svg(kind, size, accent).into();
                        let mut tree = Tree::new(element.as_widget());
                        let node = element
                            .as_widget_mut()
                            .layout(
                                &mut tree,
                                &renderer,
                                &layout::Limits::new(Size::ZERO, bounds.size()),
                            )
                            .move_to(Point::new(
                                column as f32 * 96.0 + (96.0 - size) / 2.0,
                                row as f32 * 72.0 + 16.0,
                            ));
                        element.as_widget().draw(
                            &tree,
                            &mut renderer,
                            &Theme::Dark,
                            &iced::advanced::renderer::Style {
                                text_color: Color::WHITE,
                            },
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &bounds,
                        );
                    }
                }
                let width = (bounds.width * scale) as u32;
                let height = (bounds.height * scale) as u32;
                let pixels = renderer.screenshot(Size::new(width, height), scale, background);
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[..3] == accent_rgb),
                    "folder lost the theme accent on {name} at {scale}x"
                );
                // The type colors must survive the actual widget renderer without symbolic tinting.
                for color in [
                    [133, 143, 155],
                    [107, 134, 154],
                    [86, 124, 181],
                    [191, 104, 104],
                    [78, 138, 112],
                    [136, 112, 174],
                    [176, 130, 79],
                ] {
                    assert!(
                        pixels.as_chunks::<4>().0.iter().any(|p| p[..3] == color),
                        "missing {color:?} on {name} at {scale}x"
                    );
                }
                if let Some(directory) = std::env::var_os("WADDLE_RENDER_ARTIFACT_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    image::save_buffer(
                        std::path::PathBuf::from(directory)
                            .join(format!("files-{name}-{scale}.png")),
                        &pixels,
                        width,
                        height,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    });
}
