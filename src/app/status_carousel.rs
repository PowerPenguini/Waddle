use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, renderer,
        widget::{Tree, tree},
    },
    mouse,
    time::{Duration, Instant},
    window,
};

const SPEED: f32 = 32.0;
const GAP: f32 = 48.0;
const PAUSE: f32 = 2.0;

pub(super) fn status_carousel<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    text: &'a str,
    reduced_motion: bool,
) -> Element<'a, Message> {
    Element::new(Carousel {
        content: content.into(),
        text,
        reduced_motion,
    })
}

struct Carousel<'a, Message> {
    content: Element<'a, Message>,
    text: &'a str,
    reduced_motion: bool,
}

#[derive(Default)]
struct State {
    text: String,
    started: Option<Instant>,
    elapsed: f32,
    width: f32,
}

impl State {
    fn reset(&mut self) {
        self.started = None;
        self.elapsed = 0.0;
    }
}

fn offset(elapsed: f32, width: f32) -> f32 {
    let distance = width + GAP;
    let phase = elapsed % (PAUSE + distance / SPEED);
    (phase - PAUSE).max(0.0) * SPEED
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Carousel<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            text: self.text.to_owned(),
            ..State::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
        let state = tree.state.downcast_mut::<State>();
        if state.text != self.text || self.reduced_motion {
            state.text = self.text.to_owned();
            state.reset();
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height)),
        );
        let size = limits.resolve(Length::Fill, Length::Shrink, child.size());
        let state = tree.state.downcast_mut::<State>();
        if state.width != size.width {
            state.width = size.width;
            state.reset();
        }
        layout::Node::with_children(size, vec![child])
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let width = layout.children().next().unwrap().bounds().width;
        if self.reduced_motion || width <= layout.bounds().width {
            state.reset();
            return;
        }
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let started = *state.started.get_or_insert(*now);
            state.elapsed = now.saturating_duration_since(started).as_secs_f32();
            shell.request_redraw_at(*now + Duration::from_millis(33));
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let Some(clip) = layout.bounds().intersection(viewport) else {
            return;
        };
        let child = layout.children().next().unwrap();
        let width = child.bounds().width;
        let moving = !self.reduced_motion && width > layout.bounds().width;
        let shift = if moving {
            offset(tree.state.downcast_ref::<State>().elapsed, width)
        } else {
            0.0
        };
        renderer.with_layer(clip, |renderer| {
            for translation in [Some(-shift), moving.then_some(width + GAP - shift)]
                .into_iter()
                .flatten()
            {
                renderer.with_translation(Vector::new(translation, 0.0), |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        child,
                        mouse::Cursor::Unavailable,
                        &Rectangle {
                            x: clip.x - translation,
                            ..clip
                        },
                    );
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carousel_pauses_then_moves_slowly_and_wraps_without_a_jump() {
        assert_eq!(offset(0.0, 300.0), 0.0);
        assert_eq!(offset(1.9, 300.0), 0.0);
        assert_eq!(offset(3.0, 300.0), 32.0);
        let cycle = PAUSE + (300.0 + GAP) / SPEED;
        assert!((offset(cycle - 0.01, 300.0) - 347.68).abs() < 0.01);
        assert!(offset(cycle + 1.0, 300.0) < 0.01);
    }
    #[test]
    fn resetting_text_or_width_returns_to_the_start() {
        let mut state = State {
            started: Some(Instant::now()),
            elapsed: 10.0,
            ..State::default()
        };
        state.reset();
        assert!(state.started.is_none());
        assert_eq!(state.elapsed, 0.0);
    }
    #[test]
    #[ignore = "requires a headless wgpu adapter"]
    fn carousel_layout_animates_only_overflow_and_renders_clipped_text() {
        use iced::advanced::{Renderer as _, renderer::Headless};
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut renderer = <iced::Renderer as Headless>::new(
                    iced::Font::DEFAULT,
                    iced::Pixels(11.0),
                    Some("wgpu"),
                )
                .await
                .unwrap();
                for (label, text, reduced, should_move) in [
                    (
                        "scrolling",
                        "46 items  •  /home/powerpenguini/Projects/Waddle/a-long-folder-name",
                        false,
                        true,
                    ),
                    ("short", "46 items", false, false),
                    (
                        "reduced",
                        "46 items  •  /home/powerpenguini/Projects/Waddle/a-long-folder-name",
                        true,
                        false,
                    ),
                ] {
                    let mut element: Element<'_, ()> = status_carousel(
                        iced::widget::text(text)
                            .size(11)
                            .font(iced::Font::MONOSPACE)
                            .wrapping(iced::advanced::text::Wrapping::None),
                        text,
                        reduced,
                    );
                    let mut tree = Tree::new(element.as_widget());
                    let size = Size::new(240.0, 16.0);
                    let bounds = Rectangle::with_size(size);
                    let node = element.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, size),
                    );
                    assert_eq!(node.size().width, 240.0);
                    let now = Instant::now();
                    for seconds in [0, 10] {
                        let mut messages = Vec::new();
                        let mut shell = Shell::new(&mut messages);
                        element.as_widget_mut().update(
                            &mut tree,
                            &Event::Window(window::Event::RedrawRequested(
                                now + Duration::from_secs(seconds),
                            )),
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut shell,
                            &bounds,
                        );
                        assert_eq!(
                            shell.redraw_request() != window::RedrawRequest::Wait,
                            should_move
                        );
                        renderer.reset(bounds);
                        element.as_widget().draw(
                            &tree,
                            &mut renderer,
                            &iced::Theme::Dark,
                            &renderer::Style {
                                text_color: iced::Color::WHITE,
                            },
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &bounds,
                        );
                        let pixels =
                            renderer.screenshot(Size::new(240, 16), 1.0, iced::Color::BLACK);
                        if let Ok(directory) = std::env::var("WADDLE_CAROUSEL_SCREENSHOTS") {
                            image::save_buffer(
                                std::path::Path::new(&directory)
                                    .join(format!("carousel-{label}-{seconds}.png")),
                                &pixels,
                                240,
                                16,
                                image::ColorType::Rgba8,
                            )
                            .unwrap();
                        }
                    }
                    assert_eq!(
                        tree.state.downcast_ref::<State>().elapsed > 0.0,
                        should_move
                    );
                }
            });
    }
}
