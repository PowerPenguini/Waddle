use std::{
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant as StdInstant},
};

use iced::{Point, Size, event, mouse, time::Instant};

use super::{view::View, *};

const SAMPLE_COUNT: usize = 21;

fn entry(index: usize) -> FileEntry {
    let name = format!("benchmark-entry-{index}.txt");
    FileEntry {
        path: PathBuf::from("/benchmark").join(&name),
        name: name.into(),
        directory: false,
        metadata: Default::default(),
    }
}

fn app_with_entries(list: bool) -> App {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries((0..10_000).map(entry).collect());
    app.grid.resize(Size::new(1_920.0, 1_050.0));
    let current = app.navigation.current().to_path_buf();
    app.view_preferences
        .apply_command(&current, true, if list { "view=list" } else { "view=grid" })
        .unwrap();
    app.grid.set_list_mode(list);
    app
}

fn benchmark(
    label: &str,
    iterations: usize,
    p95_budget: Duration,
    mut operation: impl FnMut(usize),
) {
    for index in 0..iterations.min(100) {
        operation(black_box(index));
    }
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    for _ in 0..SAMPLE_COUNT {
        let started = StdInstant::now();
        for index in 0..iterations {
            operation(black_box(index));
        }
        samples.push(started.elapsed() / iterations as u32);
    }
    samples.sort_unstable();
    let median = samples[SAMPLE_COUNT / 2];
    let p95 = samples[SAMPLE_COUNT * 19 / 20];
    println!(
        "benchmark {label}: median={median:?}/op p95={p95:?}/op budget={p95_budget:?}/op samples={SAMPLE_COUNT} iterations={iterations}"
    );
    assert!(
        p95 <= p95_budget,
        "{label} p95 {p95:?}/op exceeded the {p95_budget:?}/op performance budget"
    );
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_grid_cursor_and_view_work() {
    let mut app = app_with_entries(false);
    benchmark("grid-cursor-view", 500, Duration::from_millis(8), |index| {
        let position = Point::new(260.0 + (index % 1_500) as f32, 120.0 + (index % 800) as f32);
        let task = app.handle_event(
            iced::Event::Mouse(mouse::Event::CursorMoved { position }),
            event::Status::Ignored,
        );
        let _ = black_box(task);
        black_box(View::new(&app).render());
    });
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_list_cursor_and_view_work() {
    let mut app = app_with_entries(true);
    benchmark("list-cursor-view", 500, Duration::from_millis(8), |index| {
        let position = Point::new(260.0 + (index % 1_500) as f32, 80.0 + (index % 900) as f32);
        let task = app.handle_event(
            iced::Event::Mouse(mouse::Event::CursorMoved { position }),
            event::Status::Ignored,
        );
        let _ = black_box(task);
        black_box(View::new(&app).render());
    });
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_internal_drag_preview_work() {
    let mut app = app_with_entries(false);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    app.transfers
        .press(0, Point::ORIGIN, app.navigation.entries().len());
    app.transfers.move_pointer(
        Point::new(6.0, 0.0),
        app.navigation.entries(),
        app.grid.selected_indices(),
    );
    benchmark(
        "internal-drag-preview",
        2_000,
        Duration::from_micros(100),
        |index| {
            let position = Point::new(300.0 + (index % 1_000) as f32, 200.0);
            let _ = black_box(app.handle_event(
                iced::Event::Mouse(mouse::Event::CursorMoved { position }),
                event::Status::Ignored,
            ));
            black_box(View::drag_preview_layer(&app));
        },
    );
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_scroll_frame_work() {
    benchmark("scroll-frame", 10_000, Duration::from_micros(20), |index| {
        let now = Instant::now();
        let mut grid = GridInteraction::default();
        grid.observe_scroll(ScrollTarget::Entries, 100.0, 10_000.0, now, 10_000);
        grid.wheel_scroll(
            ScrollTarget::Entries,
            mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            false,
            true,
            now,
        );
        black_box(grid.tick_scroll(now + Duration::from_millis((index % 100) as u64)));
    });
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_large_marquee_selection_work() {
    benchmark(
        "large-marquee-selection",
        500,
        Duration::from_micros(250),
        |index| {
            let mut grid = GridInteraction::default();
            grid.resize(Size::new(1_920.0, 1_050.0));
            let start = Point::new(240.0, 90.0);
            grid.start_marquee(start, 10_000, 25.0, true);
            grid.move_cursor(Point::new(1_800.0, 900.0 + (index % 100) as f32), 10_000);
            black_box(grid.selection_count());
        },
    );
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_cached_theme_and_sidebar_loading_work() {
    let (app, _) = App::new();
    benchmark("cached-theme", 100_000, Duration::from_micros(2), |_| {
        black_box(app.iced_theme());
    });
    benchmark("sidebar-loading", 100_000, Duration::from_micros(2), |_| {
        black_box(app.sidebar_tree.has_loading());
    });
}
