//! Shared business rule for daily required-work deficits.
//!
//! Views must receive this result from a command; they must not reproduce the
//! subtraction in Svelte with independently cached schedule data.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeLoss {
    pub seconds: i64,
}

/// Calculates a member's remaining required work for the current day.
/// External staff have no daily requirement regardless of the schedule.
pub fn calculate(required_seconds: i64, worked_seconds: i64, is_external_staff: bool) -> TimeLoss {
    TimeLoss {
        seconds: if is_external_staff {
            0
        } else {
            (required_seconds - worked_seconds).max(0)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::calculate;

    #[test]
    fn never_returns_a_negative_deficit() {
        assert_eq!(calculate(8 * 3600, 9 * 3600, false).seconds, 0);
    }

    #[test]
    fn external_staff_have_no_deficit() {
        assert_eq!(calculate(8 * 3600, 0, true).seconds, 0);
    }
}
