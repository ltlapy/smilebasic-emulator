//! Version profiles. The runtime implements a superset of SmileBASIC 4;
//! each profile maps one SmileBASIC version's commands and screen layout onto it.

/// A SmileBASIC version to emulate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// SmileBASIC 4 (Nintendo Switch).
    #[default]
    Sb4,
    /// SmileBASIC 3 (Nintendo 3DS / Wii U).
    Sb3,
    /// Petit Computer / Petit Computer mkII (SmileBASIC v1/v2, Nintendo DSi).
    Ptc,
}

/// Initial screen configuration of a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenConfig {
    pub width: u32,
    pub height: u32,
    /// Window scale used by the frontend when the window first opens.
    pub default_scale: u32,
}

impl Profile {
    pub fn screen(self) -> ScreenConfig {
        match self {
            // ACLS resets the screen with `XSCREEN 400,240,2`.
            Profile::Sb4 => ScreenConfig {
                width: 400,
                height: 240,
                default_scale: 3,
            },
            // Top screen only for now; the 320x240 lower screen comes later.
            Profile::Sb3 => ScreenConfig {
                width: 400,
                height: 240,
                default_scale: 3,
            },
            Profile::Ptc => ScreenConfig {
                width: 256,
                height: 192,
                default_scale: 4,
            },
        }
    }
}
