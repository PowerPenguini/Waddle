use super::*;
use crate::app::Motion;
use iced::advanced::Renderer as _;
use iced::advanced::{Layout, layout, renderer::Headless, widget::Tree};
use iced::{Point, Rectangle, Size};
use std::collections::BTreeSet;

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
