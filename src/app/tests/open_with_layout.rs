use super::*;
use iced::advanced::{
    Layout, layout,
    renderer::{Headless, Renderer as _},
    widget::{Operation, Tree},
};
use iced::{Point, Rectangle, Size, mouse};

#[derive(Default)]
struct TextBounds(Vec<(String, Rectangle)>);

impl Operation for TextBounds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.to_owned(), bounds));
    }
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn open_with_rows_align_and_ignore_clicks() {
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
        let (mut app, _) = crate::app::App::new();
        app.open_with = open_with::Session::with_applications(
            "/work/document.md".into(),
            vec![
                open_with::Application {
                    name: "Omawrite".into(),
                    id: "omawrite.desktop".into(),
                    default: true,
                },
                open_with::Application {
                    name: "LibreOffice Writer".into(),
                    id: "libreoffice-writer.desktop".into(),
                    default: false,
                },
                open_with::Application {
                    name: "Neovim".into(),
                    id: "nvim.desktop".into(),
                    default: false,
                },
            ],
        );
        app.open_with.move_selection(3);
        for width in [640.0, 1000.0] {
            let size = Size::new(width, 200.0);
            let mut element = View::new(&app).open_with_bar();
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            let mut texts = TextBounds::default();
            element
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut texts);
            let ids: Vec<_> = texts
                .0
                .iter()
                .filter(|(text, _)| text.ends_with(".desktop"))
                .collect();
            assert_eq!(ids.len(), 3);
            let name_bounds = |name: &str| {
                texts
                    .0
                    .iter()
                    .find(|(text, _)| text == name)
                    .expect("list label")
                    .1
            };
            let previous = name_bounds("LibreOffice Writer");
            let last_app = name_bounds("Neovim");
            let custom = name_bounds("Custom app...");
            assert!((custom.x - last_app.x).abs() < 0.01);
            assert!(
                (custom.y - last_app.y - (last_app.y - previous.y)).abs() < 0.01,
                "Custom app must immediately follow the application rows"
            );
            let mut messages = Vec::new();
            for bounds in ids.iter().map(|(_, bounds)| *bounds).chain([custom]) {
                let cursor =
                    mouse::Cursor::Available(Point::new(bounds.x + 5.0, bounds.center_y()));
                for event in [
                    mouse::Event::ButtonPressed(mouse::Button::Left),
                    mouse::Event::ButtonReleased(mouse::Button::Left),
                ] {
                    element.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Mouse(event),
                        Layout::new(&node),
                        cursor,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut iced::advanced::Shell::new(&mut messages),
                        &Rectangle::with_size(size),
                    );
                }
            }
            assert!(
                messages.is_empty(),
                "application rows must not handle clicks: {messages:?}"
            );
            for (_, bounds) in &ids[1..] {
                assert!(
                    (bounds.x - ids[0].1.x).abs() < 0.01,
                    "desktop IDs must share a column at width {width}: {ids:?}"
                );
            }
            if let Some(directory) = std::env::var_os("WADDLE_RENDER_ARTIFACT_DIR") {
                let bounds = Rectangle::with_size(size);
                renderer.reset(bounds);
                element.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &app.iced_theme(),
                    &iced::advanced::renderer::Style {
                        text_color: Color::WHITE,
                    },
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &bounds,
                );
                let pixels = renderer.screenshot(
                    Size::new(width as u32, size.height as u32),
                    1.0,
                    Color::from_rgb8(35, 35, 35),
                );
                std::fs::create_dir_all(&directory).unwrap();
                image::save_buffer(
                    std::path::PathBuf::from(directory).join(format!("open-with-{width}.png")),
                    &pixels,
                    width as u32,
                    size.height as u32,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    });
}
