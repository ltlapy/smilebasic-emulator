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
    /// XSCREEN's sample magnification: each pixel is drawn as this many
    /// finer ones across and down.
    pub sample: u32,
    /// Size of the console's video output, used for the window when it
    /// first opens. The screen is scaled up inside it by a whole factor.
    pub display: (u32, u32),
}

impl Profile {
    pub fn screen(self) -> ScreenConfig {
        match self {
            // ACLS resets the screen with `XSCREEN 400,240,2`.
            // The Switch outputs 1280x720.
            Profile::Sb4 => ScreenConfig {
                width: 400,
                height: 240,
                sample: 2,
                display: (1280, 720),
            },
            // Top screen only for now; the 320x240 lower screen comes later.
            Profile::Sb3 => ScreenConfig {
                width: 400,
                height: 240,
                sample: 1,
                display: (1200, 720),
            },
            Profile::Ptc => ScreenConfig {
                width: 256,
                height: 192,
                sample: 1,
                display: (1024, 768),
            },
        }
    }
}
