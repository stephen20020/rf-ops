//! Manual battery verification rules.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryResult {
    /// New/fully charged battery fitted.
    Fresh,
    /// Checked and acceptable.
    Ok,
    /// Battery replaced.
    Replaced,
    /// Checked and found low; needs replacing.
    Low,
}

impl BatteryResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Ok => "ok",
            Self::Replaced => "replaced",
            Self::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fresh" => Some(Self::Fresh),
            "ok" => Some(Self::Ok),
            "replaced" => Some(Self::Replaced),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryCheckRef {
    pub result: BatteryResult,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryRule {
    /// A check is required after this time (typically the show's call time). `None` disables.
    pub required_since: Option<DateTime<Utc>>,
    /// Maximum age of the last check. `None` disables.
    pub max_age_minutes: Option<i64>,
    /// Warn this many minutes before `max_age_minutes` expires.
    pub warn_before_minutes: i64,
}

impl Default for BatteryRule {
    fn default() -> Self {
        Self {
            required_since: None,
            max_age_minutes: Some(240),
            warn_before_minutes: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryDue {
    Ok,
    DueSoon,
    Overdue,
    /// Last check found the battery low.
    Low,
    /// Never checked.
    Unchecked,
}

pub fn battery_due(
    last: Option<BatteryCheckRef>,
    rule: &BatteryRule,
    now: DateTime<Utc>,
) -> BatteryDue {
    let Some(last) = last else {
        return BatteryDue::Unchecked;
    };
    if last.result == BatteryResult::Low {
        return BatteryDue::Low;
    }
    if let Some(since) = rule.required_since {
        if now >= since && last.checked_at < since {
            return BatteryDue::Overdue;
        }
    }
    if let Some(max) = rule.max_age_minutes {
        let expires = last.checked_at + Duration::minutes(max);
        if now >= expires {
            return BatteryDue::Overdue;
        }
        if now >= expires - Duration::minutes(rule.warn_before_minutes) {
            return BatteryDue::DueSoon;
        }
    }
    BatteryDue::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, h, m, 0).unwrap()
    }

    fn check(result: BatteryResult, at: DateTime<Utc>) -> Option<BatteryCheckRef> {
        Some(BatteryCheckRef {
            result,
            checked_at: at,
        })
    }

    #[test]
    fn unchecked() {
        assert_eq!(
            battery_due(None, &BatteryRule::default(), t(12, 0)),
            BatteryDue::Unchecked
        );
    }

    #[test]
    fn low_overrides_everything() {
        assert_eq!(
            battery_due(
                check(BatteryResult::Low, t(12, 0)),
                &BatteryRule::default(),
                t(12, 1)
            ),
            BatteryDue::Low
        );
    }

    #[test]
    fn age_based() {
        let r = BatteryRule {
            required_since: None,
            max_age_minutes: Some(120),
            warn_before_minutes: 15,
        };
        let c = check(BatteryResult::Fresh, t(10, 0));
        assert_eq!(battery_due(c, &r, t(11, 0)), BatteryDue::Ok);
        assert_eq!(battery_due(c, &r, t(11, 45)), BatteryDue::DueSoon);
        assert_eq!(battery_due(c, &r, t(12, 0)), BatteryDue::Overdue);
    }

    #[test]
    fn required_since_call_time() {
        let r = BatteryRule {
            required_since: Some(t(17, 0)),
            max_age_minutes: None,
            warn_before_minutes: 0,
        };
        // checked yesterday-ish (before call)
        assert_eq!(
            battery_due(check(BatteryResult::Ok, t(9, 0)), &r, t(16, 0)),
            BatteryDue::Ok
        );
        assert_eq!(
            battery_due(check(BatteryResult::Ok, t(9, 0)), &r, t(17, 30)),
            BatteryDue::Overdue
        );
        assert_eq!(
            battery_due(check(BatteryResult::Replaced, t(17, 10)), &r, t(17, 30)),
            BatteryDue::Ok
        );
    }

    #[test]
    fn parse_roundtrip() {
        for r in [
            BatteryResult::Fresh,
            BatteryResult::Ok,
            BatteryResult::Replaced,
            BatteryResult::Low,
        ] {
            assert_eq!(BatteryResult::parse(r.as_str()), Some(r));
        }
    }
}
