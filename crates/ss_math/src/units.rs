use serde::{Deserialize, Serialize};

/// One Roblox stud in meters (Roblox physics units: 1 stud = 28 cm).
pub const STUD_IN_METERS: f32 = 0.28;

/// What one scene unit means. Stored in the project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Units {
    #[default]
    Studs,
    Meters,
}

impl Units {
    pub fn short(self) -> &'static str {
        match self {
            Units::Studs => "studs",
            Units::Meters => "m",
        }
    }

    /// Human label for a length, e.g. `6 studs`, `1 stud`, `0.5 studs`, `2.25 m`.
    pub fn format(self, value: f32) -> String {
        let v = format_number(value);
        match self {
            Units::Studs if v == "1" => "1 stud".to_owned(),
            Units::Studs => format!("{v} studs"),
            Units::Meters => format!("{v} m"),
        }
    }
}

/// Up to 2 decimals, without trailing zeros, and never `-0`.
pub fn format_number(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    let s = format!("{rounded:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(Units::Studs.format(6.0), "6 studs");
        assert_eq!(Units::Studs.format(1.0), "1 stud");
        assert_eq!(Units::Studs.format(0.5), "0.5 studs");
        assert_eq!(Units::Meters.format(2.254), "2.25 m");
        assert_eq!(format_number(-0.001), "0");
    }
}
