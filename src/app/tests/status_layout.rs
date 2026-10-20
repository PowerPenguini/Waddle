use super::*;
use iced::Size;
use iced::advanced::{layout, renderer::Headless, widget::Tree};

fn tallest_leaf(node: &layout::Node) -> f32 {
    if node.children().is_empty() {
        node.size().height
    } else {
        node.children().iter().map(tallest_leaf).fold(0.0, f32::max)
    }
}

fn assert_single_status_line(app: &crate::app::App, renderer: &iced::Renderer) {
    let mut element = View::new(app).status_bar();
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(584.0, 25.0)),
    );
    let height = tallest_leaf(&node);
    assert!(
        height > 0.0 && height <= 15.1,
        "status expanded into multiple lines: {height}"
    );
}

#[test]
#[ignore = "requires a headless wgpu adapter"]
fn filenames_and_notices_with_line_breaks_stay_on_one_status_line() {
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
                "first\u{b}second.txt",
                "first\u{c}second.txt",
                "first\u{85}second.txt",
            ] {
                let temp = tempfile::tempdir().unwrap();
                std::fs::write(temp.path().join(name), "fixture").unwrap();
                let (mut app, _) = crate::app::App::new();
                app.navigation =
                    super::super::navigation::NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(crate::fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 1);
                app.presentation.clear_notice();
                app.refresh_status();
                for notice in [false, true] {
                    if notice {
                        app.presentation
                            .set_notice(format!("Could not open {name}"));
                    }
                    assert!(app.browser_status_model().text.contains(name));
                    assert_single_status_line(&app, &renderer);
                }
                let destination = temp.path().join("destination");
                std::fs::create_dir(&destination).unwrap();
                std::fs::write(destination.join(name), "existing contents").unwrap();
                app.transfers = super::super::transfer_session::TransferSession::open(
                    temp.path().join("transfers.json"),
                );
                let request = crate::transfer::Request::detached_clipboard(
                    crate::transfer::ClipboardImport {
                        paths: vec![temp.path().join(name)],
                        action: crate::transfer::Action::Copy,
                        generation: None,
                    },
                    destination.clone(),
                )
                .unwrap();
                use iced::futures::StreamExt;
                let mut tasks = vec![app.start_transfer(request)];
                while let Some(task) = tasks.pop() {
                    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                        while let Some(action) = stream.next().await {
                            if let iced_runtime::Action::Output(message) = action {
                                tasks.push(app.update(message));
                            }
                        }
                    }
                }
                assert_eq!(
                    app.browser_status_model().presentation,
                    BrowserStatusPresentation::Conflict
                );
                assert_single_status_line(&app, &renderer);
                assert_eq!(
                    std::fs::read_to_string(destination.join(name)).unwrap(),
                    "existing contents"
                );
                assert!(
                    temp.path().join(name).is_file(),
                    "displaying a filename must not rewrite it"
                );
            }
        });
}
