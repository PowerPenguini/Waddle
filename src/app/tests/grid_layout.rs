use super::*;
use crate::app::Motion;
use iced::advanced::Renderer as _;
use iced::advanced::{
    Layout, layout,
    renderer::Headless,
    widget::{Operation, Tree},
};
use iced::{Point, Rectangle, Size};
use std::collections::BTreeSet;

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

fn tile_bounds(layout: Layout<'_>, size: Size, tiles: &mut Vec<Rectangle>) {
    let bounds = layout.bounds();
    if (bounds.width - size.width).abs() < 0.01 && (bounds.height - size.height).abs() < 0.01 {
        tiles.push(bounds);
    } else {
        for child in layout.children() {
            tile_bounds(child, size, tiles);
        }
    }
}

fn redraw(
    body: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &iced::Renderer,
    window: Size,
) -> Vec<Message> {
    let mut messages = Vec::new();
    body.as_widget_mut().update(
        tree,
        &iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now())),
        Layout::new(node),
        iced::mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut iced::advanced::Shell::new(&mut messages),
        &Rectangle::with_size(window),
    );
    messages
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn marquee_selects_the_tiles_inside_the_rendered_rectangle() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let renderer = <iced::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("wgpu"),
        )
        .await
        .expect("headless wgpu adapter");
        let temp = tempfile::tempdir().unwrap();
        let (mut app, _) = App::new();
        app.view_preferences =
            super::super::view_preferences::Preferences::empty_at(temp.path().join("preferences"));
        app.navigation.replace_displayed_entries(
            (0..17)
                .map(|index| FileEntry {
                    path: temp.path().join(format!("file-{index}")),
                    name: format!("file-{index}").into(),
                    directory: false,
                    metadata: Default::default(),
                })
                .collect(),
        );
        let window = Size::new(1000.0, 700.0);
        app.grid.set_sidebar_visible(true);
        app.grid.set_list_mode(false);
        app.grid.set_icon_size(40);
        // Scroll in a short window, then enlarge it until every file fits.
        let short_window = Size::new(window.width, 260.0);
        app.grid.resize(short_window);
        let mut body = View::new(&app).render();
        let mut tree = Tree::new(body.as_widget());
        let node = body.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, short_window),
        );
        body.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut iced::advanced::widget::operation::scrollable::scroll_to::<()>(
                Id::new(GRID_SCROLL_ID),
                scrollable::AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(app.grid.tile_row_height()),
                },
            ),
        );
        let messages = redraw(&mut body, &mut tree, &node, &renderer, short_window);
        drop(body);
        for message in messages {
            if let Message::Scrolled { .. } = message {
                let _ = app.update(message);
            }
        }
        assert!(app.grid.scroll_offset() > 0.0);
        app.grid.resize(window);
        let mut body = View::new(&app).render();
        tree.diff(body.as_widget());
        let node = body.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, window),
        );
        let mut tiles = Vec::new();
        tile_bounds(
            Layout::new(&node),
            Size::new(app.grid.tile_width(), app.grid.tile_height()),
            &mut tiles,
        );
        let messages = redraw(&mut body, &mut tree, &node, &renderer, window);
        drop(body);
        for message in messages {
            if let Message::Scrolled { .. } = message {
                let _ = app.update(message);
            }
        }
        assert_eq!(tiles.len(), 17);
        let columns = app.grid.visible_range(17, app.status_height()).columns;
        let start = Point::new(tiles[3].x - 2.0, tiles[columns * 2].y - 2.0);
        let end = Point::new(tiles[1].x + 20.0, tiles[0].y + 20.0);
        assert!(app.grid.start_marquee(start, 17, app.status_height(), true));
        app.grid.move_cursor(end, 17);
        let local = app.grid.marquee_bounds(app.status_height()).unwrap();
        let rectangle = Rectangle::new(
            Point::new(local.x + SIDEBAR_WIDTH, local.y + TOOLBAR_HEIGHT + 1.0),
            local.size(),
        );
        let expected: BTreeSet<_> = tiles
            .iter()
            .enumerate()
            .filter_map(|(index, tile)| rectangle.intersects(tile).then_some(index))
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(
            app.grid.selected_indices(),
            &expected,
            "tiles: {tiles:?}, rectangle: {rectangle:?}"
        );
    });
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn visual_block_rendered_layout_matches_selected_tiles() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut renderer = <iced::Renderer as Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(14.0),
                Some("wgpu"),
            )
            .await
            .unwrap();
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            app.navigation.replace_displayed_entries(
                (0..80)
                    .map(|i| FileEntry {
                        path: std::path::PathBuf::from(format!("/start/file{i:02}.txt")),
                        name: format!("file{i:02}.txt").into(),
                        directory: false,
                        metadata: Default::default(),
                    })
                    .collect(),
            );
            app.grid.set_icon_size(48);
            app.grid.set_list_mode(false);
            app.grid.select_only(Some(1), 80);
            app.grid.toggle_visual_block(80);
            app.grid.move_selection_count(Motion::Down, 3, 80, 25.0);
            for width in [584.0, 1000.0] {
                let size = Size::new(width, 560.0);
                app.grid.resize(size);
                app.grid.select_only(Some(1), 80);
                app.grid.toggle_visual_block(80);
                app.grid.move_selection(Motion::Down, 80);
                app.grid.move_selection(Motion::Right, 80);
                app.refresh_status();
                assert!(app.presentation.status().contains("VISUAL BLOCK"));
                let bounds = Rectangle::with_size(size);
                renderer.reset(bounds);
                let mut body = View::new(&app).render();
                let mut tree = Tree::new(body.as_widget());
                let node = body.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let mut tiles = Vec::new();
                tile_bounds(
                    Layout::new(&node),
                    Size::new(app.grid.tile_width(), app.grid.tile_height()),
                    &mut tiles,
                );
                let selected: Vec<_> = app
                    .grid
                    .selected_indices()
                    .iter()
                    .map(|&i| tiles[i])
                    .collect();
                assert_eq!(selected.len(), 4);
                let left = selected.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
                let top = selected.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
                let right = selected.iter().map(|r| r.x + r.width).fold(0.0, f32::max);
                let bottom = selected.iter().map(|r| r.y + r.height).fold(0.0, f32::max);
                let rectangle =
                    Rectangle::new(Point::new(left, top), Size::new(right - left, bottom - top));
                let expected: BTreeSet<_> = tiles
                    .iter()
                    .enumerate()
                    .filter_map(|(i, tile)| rectangle.intersects(tile).then_some(i))
                    .collect();
                assert_eq!(app.grid.selected_indices(), &expected);
                body.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &iced::Theme::Dark,
                    &iced::advanced::renderer::Style {
                        text_color: iced::Color::WHITE,
                    },
                    Layout::new(&node),
                    iced::mouse::Cursor::Unavailable,
                    &bounds,
                );
                let pixels =
                    renderer.screenshot(Size::new(width as u32, 560), 1.0, iced::Color::BLACK);
                if let Ok(directory) = std::env::var("WADDLE_BLOCK_SCREENSHOTS") {
                    image::save_buffer(
                        std::path::Path::new(&directory).join(format!("block-{width}.png")),
                        &pixels,
                        width as u32,
                        560,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        });
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn filenames_with_line_breaks_keep_list_cells_on_one_line() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let renderer = <iced::Renderer as Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(14.0),
                Some("wgpu"),
            )
            .await
            .expect("headless wgpu adapter");
            for name in [
                "first\nsecond.txt",
                "first\rsecond.txt",
                "first\u{2028}second.txt",
                "first\u{2029}second.txt",
                "first.txt\npng",
                "first\u{b}second.txt",
                "first\u{c}second.txt",
                "first\u{85}second.txt",
                "first.averyveryveryveryveryveryverylongextension",
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join(name);
                std::fs::write(&path, "contents").unwrap();
                let (mut app, _) = App::new();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.set_list_mode(true);
                app.grid.resize(Size::new(1000.0, 700.0));
                for icon_size in [24, 48, 128] {
                    app.grid.set_icon_size(icon_size);
                    let mut row = View::new(&app).file_list_row(0);
                    let mut tree = Tree::new(row.as_widget());
                    let node = row.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, Size::new(1000.0, 200.0)),
                    );
                    let mut texts = TextBounds::default();
                    row.as_widget_mut().operate(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &mut texts,
                    );
                    assert!(texts.0.iter().any(|(text, _)| text.starts_with("first")));
                    for (text, bounds) in texts.0 {
                        assert!(
                            !text.chars().any(|character| character.is_control()
                                || matches!(character, '\u{2028}' | '\u{2029}')),
                            "list cell retains a line-break character: {text:?}"
                        );
                        assert!(
                            bounds.height <= app.grid.label_font_size() * 1.5,
                            "list cell {text:?} spans multiple lines at icon size {icon_size}: {}",
                            bounds.height
                        );
                    }
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), "contents");
                    assert_eq!(app.navigation.entries()[0].path, path);
                }
            }
        });
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn sidebar_folder_labels_with_line_breaks_fit_one_line() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let renderer = <iced::Renderer as Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(14.0),
                Some("wgpu"),
            )
            .await
            .expect("headless wgpu adapter");
            for name in [
                "first\nsecond",
                "first\rsecond",
                "first\u{2028}second",
                "first\u{2029}second",
                "first\u{b}second",
                "first\u{c}second",
                "first\u{85}second",
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join(name);
                std::fs::create_dir(&path).unwrap();
                let (mut app, _) = App::new();
                app.sidebar_tree
                    .install_locations(vec![super::super::places::Entry {
                        path: path.clone(),
                        label: name.to_owned(),
                        kind: tree::NodeKind::Favorite,
                        favorite_index: Some(0),
                    }]);
                let entry = app
                    .sidebar_tree
                    .rows(temp.path())
                    .into_iter()
                    .find(|entry| entry.path.as_ref() == Some(&path))
                    .unwrap();
                assert_eq!(entry.label, name);
                let mut row = View::new(&app).tree_row(entry);
                let mut state = Tree::new(row.as_widget());
                let node = row.as_widget_mut().layout(
                    &mut state,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(SIDEBAR_WIDTH, 100.0)),
                );
                let mut texts = TextBounds::default();
                row.as_widget_mut()
                    .operate(&mut state, Layout::new(&node), &renderer, &mut texts);
                let (text, bounds) = texts
                    .0
                    .iter()
                    .find(|(text, _)| text.starts_with("first"))
                    .unwrap();
                assert!(
                    bounds.height <= 16.1,
                    "sidebar label {text:?} spans multiple lines: {}",
                    bounds.height
                );
                assert!(!text.chars().any(|character| character.is_control()
                    || matches!(character, '\u{2028}' | '\u{2029}')));
                assert!(path.is_dir());
            }
        });
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn long_type_labels_do_not_draw_over_neighboring_list_columns() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut renderer = <iced::Renderer as Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(14.0),
                Some("wgpu"),
            )
            .await
            .expect("headless wgpu adapter");
            let temp = tempfile::tempdir().unwrap();
            let short = temp.path().join("example.txt");
            let long = temp.path().join(format!("example.{}", "W".repeat(80)));
            for path in [&short, &long] {
                std::fs::write(path, "x").unwrap();
            }
            let entries = fs::read_directory(temp.path()).unwrap();
            let metadata = entries[0].metadata;
            let (mut app, _) = App::new();
            app.grid.set_list_mode(true);
            app.grid.set_sidebar_visible(false);
            app.grid.resize(Size::new(1200.0, 700.0));
            app.grid.set_icon_size(24);
            let window = Size::new(1200.0, 50.0);
            let bounds = Rectangle::with_size(window);
            let mut screenshots = Vec::new();
            let mut column_right = None;
            for path in [&short, &long] {
                let mut entry = entries
                    .iter()
                    .find(|entry| entry.path == *path)
                    .unwrap()
                    .clone();
                entry.metadata = metadata;
                let kind = entry.type_label();
                app.navigation.install_folder_entries(vec![entry]);
                let mut row = View::new(&app).file_list_row(0);
                let mut tree = Tree::new(row.as_widget());
                let node = row.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, window),
                );
                let mut texts = TextBounds::default();
                row.as_widget_mut()
                    .operate(&mut tree, Layout::new(&node), &renderer, &mut texts);
                let type_bounds = texts.0.iter().find(|(text, _)| text == &kind).unwrap().1;
                let right =
                    (type_bounds.x + app.grid.label_scale() * LIST_TYPE_WIDTH).ceil() as usize;
                if let Some(previous) = column_right {
                    assert_eq!(right, previous);
                }
                column_right = Some(right);
                renderer.reset(bounds);
                row.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &iced::Theme::Dark,
                    &iced::advanced::renderer::Style {
                        text_color: iced::Color::WHITE,
                    },
                    Layout::new(&node),
                    iced::mouse::Cursor::Unavailable,
                    &bounds,
                );
                screenshots.push(renderer.screenshot(Size::new(1200, 50), 1.0, iced::Color::BLACK));
            }
            let right = column_right.unwrap();
            assert!(right < 1100, "fixture must leave room for metadata columns");
            let differences = (0..50)
                .flat_map(|y| ((right + 1)..1200).map(move |x| (y * 1200 + x) * 4))
                .filter(|index| {
                    screenshots[0][*index..*index + 4] != screenshots[1][*index..*index + 4]
                })
                .count();
            assert_eq!(
                differences, 0,
                "long type label painted into neighboring columns"
            );
        });
}
