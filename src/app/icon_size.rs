use iced::mouse;

pub(super) const DEFAULT: u16 = 48;
pub(super) const MIN: u16 = 24;
pub(super) const MAX: u16 = 128;
const STEP: i32 = 8;

pub(super) fn stepped(size: u16, steps: i32) -> u16 {
    (i32::from(size) + steps.clamp(-16, 16) * STEP).clamp(i32::from(MIN), i32::from(MAX)) as u16
}

/// Accumulate precise scrolling so a touchpad gesture does not jump straight to a limit.
#[derive(Default)]
pub(super) struct WheelZoom {
    remainder: f32,
}

impl WheelZoom {
    pub(super) fn reset(&mut self) {
        self.remainder = 0.0;
    }

    pub(super) fn steps(&mut self, delta: mouse::ScrollDelta) -> i32 {
        let amount = match delta {
            mouse::ScrollDelta::Lines { y, .. } => y,
            mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
        };
        if !amount.is_finite() || amount == 0.0 {
            return 0;
        }
        if self.remainder.signum() != amount.signum() {
            self.reset();
        }
        let total = self.remainder + amount;
        self.remainder = total.fract();
        (total.trunc() as i32).clamp(-16, 16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_is_bounded_even_with_extreme_input() {
        assert_eq!(stepped(DEFAULT, 1), 56);
        assert_eq!(stepped(DEFAULT, -1), 40);
        assert_eq!(stepped(MAX, i32::MAX), MAX);
        assert_eq!(stepped(MIN, i32::MIN), MIN);
    }

    #[test]
    fn precise_zoom_accumulates_and_reverses_without_sticky_remainders() {
        let mut wheel = WheelZoom::default();
        let pixels = |y| mouse::ScrollDelta::Pixels { x: 0.0, y };
        assert_eq!(wheel.steps(pixels(25.0)), 0);
        assert_eq!(wheel.steps(pixels(25.0)), 1);
        assert_eq!(wheel.steps(pixels(-40.0)), -1);
        assert_eq!(wheel.steps(pixels(f32::NAN)), 0);
        assert_eq!(wheel.steps(pixels(30.0)), 0);
        wheel.reset();
        assert_eq!(wheel.steps(pixels(10.0)), 0);
        assert_eq!(wheel.steps(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }), 1);
    }
}
