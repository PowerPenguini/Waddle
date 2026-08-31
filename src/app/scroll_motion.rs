use std::{collections::VecDeque, time::Duration};

use iced::{Animation, animation::Easing, mouse, time::Instant};

const DURATION: Duration = Duration::from_millis(100);
const LINE_STEP: f32 = 60.0;
const OFFSET_TOLERANCE: f32 = 0.5;
const MAX_PENDING_OFFSETS: usize = 16;
const TOUCHPAD_RELEASE_DELAY: Duration = Duration::from_millis(32);
const TOUCHPAD_MAX_SAMPLE_GAP: Duration = Duration::from_millis(80);
const TOUCHPAD_MAX_FRAME: Duration = Duration::from_millis(32);
const TOUCHPAD_FRICTION: f32 = 6.0;
const TOUCHPAD_MIN_VELOCITY: f32 = 24.0;
const TOUCHPAD_MAX_VELOCITY: f32 = 4_000.0;
const TOUCHPAD_VELOCITY_BLEND: f32 = 0.65;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Command {
    By(f32),
    To(f32),
}

#[derive(Clone, Debug, Default)]
struct Momentum {
    last_input: Option<Instant>,
    last_tick: Option<Instant>,
    velocity: f32,
    samples: u8,
    offset: f32,
    coasting: bool,
}

impl Momentum {
    fn input(&mut self, amount: f32, actual: f32, now: Instant) {
        if let Some(last_input) = self.last_input {
            let elapsed = now.saturating_duration_since(last_input);
            if elapsed > Duration::ZERO && elapsed <= TOUCHPAD_MAX_SAMPLE_GAP {
                let instantaneous = (amount / elapsed.as_secs_f32())
                    .clamp(-TOUCHPAD_MAX_VELOCITY, TOUCHPAD_MAX_VELOCITY);
                self.velocity = if self.velocity.abs() <= f32::EPSILON
                    || self.velocity.signum() != instantaneous.signum()
                {
                    instantaneous
                } else {
                    self.velocity * (1.0 - TOUCHPAD_VELOCITY_BLEND)
                        + instantaneous * TOUCHPAD_VELOCITY_BLEND
                };
                self.samples = self.samples.saturating_add(1);
            } else {
                self.velocity = 0.0;
                self.samples = 1;
            }
        } else {
            self.velocity = 0.0;
            self.samples = 1;
        }

        self.last_input = Some(now);
        self.last_tick = None;
        self.offset = actual;
        self.coasting = false;
    }

    fn observe_native(&mut self, offset: f32) -> bool {
        if self.last_input.is_some() && !self.coasting {
            self.offset = offset;
            true
        } else {
            false
        }
    }

    fn tick(&mut self, now: Instant, maximum: f32) -> Option<f32> {
        let last_input = self.last_input?;
        if !self.coasting {
            if now.saturating_duration_since(last_input) < TOUCHPAD_RELEASE_DELAY {
                return None;
            }
            if self.samples < 2 || self.velocity.abs() < TOUCHPAD_MIN_VELOCITY {
                self.reset();
                return None;
            }
            self.coasting = true;
            self.last_tick = Some(last_input + TOUCHPAD_RELEASE_DELAY);
        }

        let last_tick = self.last_tick.unwrap_or(now);
        let elapsed = now
            .saturating_duration_since(last_tick)
            .min(TOUCHPAD_MAX_FRAME);
        self.last_tick = Some(now);
        if elapsed.is_zero() {
            return None;
        }

        let decay = (-TOUCHPAD_FRICTION * elapsed.as_secs_f32()).exp();
        let distance = self.velocity * (1.0 - decay) / TOUCHPAD_FRICTION;
        self.velocity *= decay;
        let next = (self.offset + distance).clamp(0.0, maximum);
        if (next - self.offset).abs() <= OFFSET_TOLERANCE {
            self.reset();
            return None;
        }

        self.offset = next;
        if self.velocity.abs() < TOUCHPAD_MIN_VELOCITY {
            self.last_input = None;
            self.last_tick = None;
            self.velocity = 0.0;
            self.samples = 0;
            self.coasting = false;
        }
        Some(next)
    }

    fn active(&self) -> bool {
        self.last_input.is_some()
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug)]
pub(super) struct Motion {
    animation: Animation<f32>,
    actual: f32,
    maximum: Option<f32>,
    pending_offsets: VecDeque<f32>,
    animation_active: bool,
    momentum: Momentum,
}

impl Default for Motion {
    fn default() -> Self {
        Self::at(0.0)
    }
}

impl Motion {
    fn at(offset: f32) -> Self {
        Self {
            animation: animation(offset),
            actual: offset,
            maximum: None,
            pending_offsets: VecDeque::new(),
            animation_active: false,
            momentum: Momentum::default(),
        }
    }

    pub(super) fn wheel(
        &mut self,
        delta: mouse::ScrollDelta,
        shift: bool,
        smooth: bool,
        now: Instant,
    ) -> Option<Command> {
        let (amount, discrete) = vertical_delta(delta, shift);
        if amount.abs() <= f32::EPSILON {
            return None;
        }
        if !smooth || !discrete {
            self.cancel();
            return Some(Command::By(amount));
        }

        let start = if self.animation_active {
            self.animation.value()
        } else {
            self.actual
        };
        self.move_to(start + amount, true, now)
    }

    pub(super) fn move_to(&mut self, target: f32, smooth: bool, now: Instant) -> Option<Command> {
        self.momentum.reset();
        let start = if self.animation_active {
            self.animation.value()
        } else {
            self.actual
        };
        let target = self.clamp(target);
        if !smooth {
            self.cancel();
            return Some(Command::To(target));
        }
        if (target - start).abs() <= f32::EPSILON {
            return None;
        }
        self.animation.go_mut(target, now);
        self.animation_active = true;
        None
    }

    pub(super) fn touchpad(
        &mut self,
        delta: mouse::ScrollDelta,
        shift: bool,
        momentum: bool,
        now: Instant,
    ) {
        let (amount, discrete) = vertical_delta(delta, shift);
        self.cancel_animation();
        if !momentum || discrete {
            self.momentum.reset();
            return;
        }
        if amount.abs() <= f32::EPSILON {
            return;
        }
        self.momentum.input(amount, self.actual, now);
    }

    pub(super) fn observe(&mut self, offset: f32, maximum: f32) {
        let maximum = maximum.max(0.0);
        let offset = offset.clamp(0.0, maximum);
        self.maximum = Some(maximum);
        self.actual = offset;

        let matching_pending = self
            .pending_offsets
            .iter()
            .position(|pending| (offset - pending).abs() <= OFFSET_TOLERANCE);
        if let Some(index) = matching_pending {
            self.pending_offsets.remove(index);
            return;
        }

        if self.momentum.observe_native(offset) {
            return;
        }

        if self.pending_offsets.is_empty() {
            if self.animation_active || self.momentum.active() {
                self.reset(offset);
            }
            return;
        }

        self.reset(offset);
    }

    pub(super) fn tick(&mut self, now: Instant) -> Option<Command> {
        if self.animation_active {
            let offset = self.animation.interpolate_with(|target| target, now);
            self.expect(offset);
            if !self.animation.is_animating(now) {
                self.animation_active = false;
            }
            return Some(Command::To(offset));
        }

        let offset = self
            .momentum
            .tick(now, self.maximum.unwrap_or(f32::INFINITY))?;
        self.expect(offset);
        Some(Command::To(offset))
    }

    pub(super) fn active(&self) -> bool {
        self.animation_active || self.momentum.active()
    }

    pub(super) fn cancel(&mut self) {
        self.reset(self.actual);
    }

    fn reset(&mut self, offset: f32) {
        self.actual = offset;
        self.cancel_animation();
        self.momentum.reset();
    }

    fn cancel_animation(&mut self) {
        self.animation = animation(self.actual);
        self.pending_offsets.clear();
        self.animation_active = false;
    }

    fn expect(&mut self, offset: f32) {
        if self.pending_offsets.len() == MAX_PENDING_OFFSETS {
            self.pending_offsets.pop_front();
        }
        self.pending_offsets.push_back(offset);
    }

    fn clamp(&self, offset: f32) -> f32 {
        offset.max(0.0).min(self.maximum.unwrap_or(f32::INFINITY))
    }
}

fn animation(offset: f32) -> Animation<f32> {
    Animation::new(offset)
        .duration(DURATION)
        .easing(Easing::EaseOut)
}

fn vertical_delta(delta: mouse::ScrollDelta, shift: bool) -> (f32, bool) {
    match delta {
        mouse::ScrollDelta::Lines { x, y } => (-if shift { x } else { y } * LINE_STEP, true),
        mouse::ScrollDelta::Pixels { x, y } => (-if shift { x } else { y }, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(y: f32) -> mouse::ScrollDelta {
        mouse::ScrollDelta::Lines { x: 0.0, y }
    }

    #[test]
    fn line_wheels_retarget_the_active_animation() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(120.0, 1_000.0);

        assert_eq!(motion.wheel(lines(-1.0), false, true, now), None);
        assert_eq!(motion.animation.value(), 180.0);
        assert_eq!(
            motion.wheel(lines(-2.0), false, true, now + Duration::from_millis(20)),
            None
        );
        assert_eq!(motion.animation.value(), 300.0);
        assert!(motion.active());
    }

    #[test]
    fn absolute_targets_retarget_the_same_animation() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(120.0, 1_000.0);

        assert_eq!(motion.move_to(300.0, true, now), None);
        assert_eq!(motion.animation.value(), 300.0);
        assert_eq!(
            motion.move_to(480.0, true, now + Duration::from_millis(20)),
            None
        );
        assert_eq!(motion.animation.value(), 480.0);
        assert!(motion.active());
    }

    #[test]
    fn immediate_absolute_targets_cancel_animation() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(120.0, 1_000.0);
        motion.move_to(300.0, true, now);

        assert_eq!(
            motion.move_to(700.0, false, now + Duration::from_millis(20)),
            Some(Command::To(700.0))
        );
        assert!(!motion.active());
    }

    #[test]
    fn an_animation_reaches_its_exact_clamped_target() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(90.0, 125.0);
        motion.wheel(lines(-1.0), false, true, now);

        assert_eq!(motion.animation.value(), 125.0);
        assert_eq!(motion.tick(now + DURATION), Some(Command::To(125.0)));
        assert!(!motion.active());
    }

    #[test]
    fn one_scroll_step_settles_within_the_interaction_latency_budget() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);
        motion.wheel(lines(-1.0), false, true, now);

        assert_eq!(
            motion.tick(now + Duration::from_millis(100)),
            Some(Command::To(160.0))
        );
        assert!(!motion.active());
    }

    #[test]
    fn pixel_wheels_and_disabled_smoothing_scroll_directly() {
        let now = Instant::now();
        let mut motion = Motion::default();

        assert_eq!(
            motion.wheel(
                mouse::ScrollDelta::Pixels { x: 0.0, y: -7.5 },
                false,
                true,
                now,
            ),
            Some(Command::By(7.5))
        );
        assert_eq!(
            motion.wheel(lines(-1.0), false, false, now),
            Some(Command::By(60.0))
        );
        assert!(!motion.active());
    }

    #[test]
    fn touchpad_motion_continues_after_the_last_precise_delta() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);

        for (elapsed_ms, offset) in [(0, 108.0), (16, 116.0), (32, 124.0)] {
            let at = now + Duration::from_millis(elapsed_ms);
            motion.touchpad(
                mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 },
                false,
                true,
                at,
            );
            motion.observe(offset, 1_000.0);
        }

        let Some(Command::To(offset)) = motion.tick(now + Duration::from_millis(96)) else {
            panic!("a released touchpad gesture should keep scrolling");
        };
        assert!(offset > 124.0);
        assert!(motion.active());
    }

    #[test]
    fn touchpad_axis_stop_keeps_the_sampled_momentum() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);

        for (elapsed_ms, offset) in [(0, 108.0), (16, 116.0), (32, 124.0)] {
            let at = now + Duration::from_millis(elapsed_ms);
            motion.touchpad(
                mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 },
                false,
                true,
                at,
            );
            motion.observe(offset, 1_000.0);
        }
        motion.touchpad(
            mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 },
            false,
            true,
            now + Duration::from_millis(48),
        );

        assert!(matches!(
            motion.tick(now + Duration::from_millis(96)),
            Some(Command::To(offset)) if offset > 124.0
        ));
    }

    #[test]
    fn touchpad_momentum_stops_after_decelerating() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);

        for (elapsed_ms, offset) in [(0, 108.0), (16, 116.0), (32, 124.0)] {
            let at = now + Duration::from_millis(elapsed_ms);
            motion.touchpad(
                mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 },
                false,
                true,
                at,
            );
            motion.observe(offset, 1_000.0);
        }

        let mut last_offset = 124.0;
        for elapsed_ms in (64..=2_000).step_by(16) {
            if let Some(Command::To(offset)) = motion.tick(now + Duration::from_millis(elapsed_ms))
            {
                last_offset = offset;
            }
        }

        assert!(last_offset > 124.0);
        assert!(!motion.active());
    }

    #[test]
    fn reduced_motion_keeps_touchpad_scrolling_direct() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);

        motion.touchpad(
            mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 },
            false,
            false,
            now,
        );
        motion.observe(108.0, 1_000.0);

        assert_eq!(motion.tick(now + Duration::from_millis(100)), None);
        assert!(!motion.active());
    }

    #[test]
    fn a_direct_widget_offset_cancels_animation() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);
        motion.wheel(lines(-1.0), false, true, now);
        assert!(motion.active());

        motion.observe(420.0, 1_000.0);

        assert!(!motion.active());
        assert_eq!(motion.actual, 420.0);
    }

    #[test]
    fn an_expected_widget_offset_keeps_animation_running() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);
        motion.wheel(lines(-1.0), false, true, now);
        let Some(Command::To(expected)) = motion.tick(now + Duration::from_millis(20)) else {
            panic!("active scrolling should produce an absolute offset");
        };

        motion.observe(expected, 1_000.0);

        assert!(motion.active());
        assert_eq!(motion.actual, expected);
    }

    #[test]
    fn a_delayed_animation_offset_does_not_cancel_newer_scroll_work() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.observe(100.0, 1_000.0);
        motion.wheel(lines(-1.0), false, true, now);
        let Some(Command::To(first)) = motion.tick(now + Duration::from_millis(16)) else {
            panic!("the first animation frame should produce an offset");
        };
        let Some(Command::To(second)) = motion.tick(now + Duration::from_millis(32)) else {
            panic!("the second animation frame should produce an offset");
        };
        assert_ne!(first, second);

        motion.observe(first, 1_000.0);

        assert!(motion.active());
        assert_eq!(motion.actual, first);
    }

    #[test]
    fn shift_uses_the_horizontal_wheel_axis() {
        let now = Instant::now();
        let mut motion = Motion::default();

        assert_eq!(
            motion.wheel(
                mouse::ScrollDelta::Lines { x: -2.0, y: 5.0 },
                true,
                false,
                now,
            ),
            Some(Command::By(120.0))
        );
    }

    #[test]
    fn widget_clamping_finishes_an_unbounded_first_scroll() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.wheel(lines(-1.0), false, true, now);
        assert_eq!(motion.tick(now + DURATION), Some(Command::To(60.0)));

        motion.observe(25.0, 25.0);

        assert!(!motion.active());
        assert_eq!(motion.animation.value(), 25.0);
    }
}
