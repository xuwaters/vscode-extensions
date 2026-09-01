//! Which language versions a builtin exists in.
//!
//! Two profiles, two vocabularies, two mask types — desktop GLSL numbers its
//! versions 1.10 … 4.60 and GLSL ES numbers its own 1.00 … 3.20, and nothing
//! good comes of letting one profile's bits be read with the other's names.
//! The masks are therefore distinct types over distinct enums.
//!
//! The bit for desktop 4.60 and the bit for ES 3.20 are *extrapolated* by the
//! generator: docs.gl's version tables stop at 4.50 and ES 3.10. Nothing was
//! removed from the language in either step, so copying the last column
//! forward is safe in the permissive direction. See
//! `docs/rfc/012-glsl-analyzer/research/docs-gl.md` §5.

/// Builds a version enum and its bitset side by side. The two profiles differ
/// only in their member list and their backing integer, so the shape is
/// written once.
macro_rules! version_set {
    (
        $(#[$enum_meta:meta])* $enum_name:ident : $bits:ty => $mask_name:ident,
        $($variant:ident = $number:literal / $label:literal),+ $(,)?
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $enum_name {
            $($variant),+
        }

        impl $enum_name {
            /// Every version, oldest first. The index into this slice is the
            /// version's bit position in the matching mask type.
            pub const ALL: &'static [$enum_name] = &[$($enum_name::$variant),+];

            /// The number a `#version` directive writes: `450`, `300`, `100`.
            pub const fn number(self) -> u16 {
                match self {
                    $($enum_name::$variant => $number),+
                }
            }

            /// The dotted form a human reads: `4.50`, `3.00`, `1.00`.
            pub const fn label(self) -> &'static str {
                match self {
                    $($enum_name::$variant => $label),+
                }
            }

            /// The version a `#version` number names, if it names one at all.
            pub fn from_number(number: u16) -> Option<$enum_name> {
                match number {
                    $($number => Some($enum_name::$variant),)+
                    _ => None,
                }
            }

            /// This version's bit position, which is also its index in
            /// [`ALL`](Self::ALL) — the variants are declared oldest first with
            /// no explicit discriminants, so the cast is the index.
            pub const fn index(self) -> u32 {
                self as u32
            }
        }

        /// A set of versions. Cheap to embed (one integer per entry) and cheap
        /// to test, which is the whole reason availability is a mask and not a
        /// range: `mix` is not contiguous in the desktop table.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
        pub struct $mask_name($bits);

        impl $mask_name {
            /// Available nowhere — what a builtin the other profile owns gets.
            pub const EMPTY: $mask_name = $mask_name(0);
            /// Available in every version this profile has.
            pub const ALL: $mask_name = $mask_name(((1 as $bits) << $enum_name::ALL.len()) - 1);

            /// The raw bits. Only the generator and its tests should care.
            pub const fn from_bits(bits: $bits) -> $mask_name {
                $mask_name(bits)
            }

            /// The raw bits, as [`from_bits`](Self::from_bits) takes them.
            pub const fn bits(self) -> $bits {
                self.0
            }

            pub const fn contains(self, version: $enum_name) -> bool {
                self.0 & (1 << version.index()) != 0
            }

            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// This set plus one more version.
            pub const fn with(self, version: $enum_name) -> $mask_name {
                $mask_name(self.0 | (1 << version.index()))
            }

            /// Every version from this one onward — how a feature that arrived
            /// and stayed is written.
            pub const fn since(version: $enum_name) -> $mask_name {
                $mask_name($mask_name::ALL.0 & !(((1 as $bits) << version.index()) - 1))
            }

            /// Every version in `first..=last` — how a feature that was later
            /// removed from core is written.
            pub const fn through(first: $enum_name, last: $enum_name) -> $mask_name {
                let above_last = $mask_name::ALL.0 & !(((1 as $bits) << (last.index() + 1)) - 1);
                $mask_name($mask_name::since(first).0 & !above_last)
            }

            /// Exactly one version.
            pub const fn only(version: $enum_name) -> $mask_name {
                $mask_name(1 << version.index())
            }

            /// Everything in either set.
            pub const fn union(self, other: $mask_name) -> $mask_name {
                $mask_name(self.0 | other.0)
            }

            /// The oldest version in the set — the "since" a hover wants.
            pub fn earliest(self) -> Option<$enum_name> {
                $enum_name::ALL.iter().copied().find(|&v| self.contains(v))
            }

            /// The newest version in the set. Paired with
            /// [`earliest`](Self::earliest) it tells a hover whether the set is
            /// a plain "since X" or something with a hole in it.
            pub fn latest(self) -> Option<$enum_name> {
                $enum_name::ALL.iter().copied().rev().find(|&v| self.contains(v))
            }

            /// Every version in the set, oldest first.
            pub fn versions(self) -> impl Iterator<Item = $enum_name> {
                $enum_name::ALL.iter().copied().filter(move |&v| self.contains(v))
            }

            /// Whether the set is every version from [`earliest`](Self::earliest)
            /// onwards with no gaps — the common case, and the one a hover can
            /// summarise as "since 4.00" instead of listing versions.
            pub fn is_contiguous_to_latest(self) -> bool {
                match self.earliest() {
                    None => false,
                    Some(first) => {
                        let below = ((1 as $bits) << first.index()) - 1;
                        self.0 == $mask_name::ALL.0 & !below
                    }
                }
            }
        }
    };
}

version_set! {
    /// A desktop OpenGL Shading Language version, as `#version` spells it.
    ///
    /// The gap between 1.50 and 3.30 is real: GLSL renumbered to match GL, so
    /// there is no 2.x, and 3.00–3.20 never existed on the desktop.
    DesktopVersion: u16 => DesktopMask,
    V110 = 110 / "1.10",
    V120 = 120 / "1.20",
    V130 = 130 / "1.30",
    V140 = 140 / "1.40",
    V150 = 150 / "1.50",
    V330 = 330 / "3.30",
    V400 = 400 / "4.00",
    V410 = 410 / "4.10",
    V420 = 420 / "4.20",
    V430 = 430 / "4.30",
    V440 = 440 / "4.40",
    V450 = 450 / "4.50",
    V460 = 460 / "4.60",
}

version_set! {
    /// A GLSL ES version, as `#version N es` spells it (1.00 is written bare,
    /// `#version 100`).
    EsVersion: u8 => EsMask,
    V100 = 100 / "1.00",
    V300 = 300 / "3.00",
    V310 = 310 / "3.10",
    V320 = 320 / "3.20",
}

/// The version a source declared, profile included — what an availability
/// question is actually asked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    Desktop(DesktopVersion),
    Es(EsVersion),
}

impl Version {
    /// The version a `#version` directive names. `es` is whether the directive
    /// carried the `es` profile token; `#version 100` is ES with or without it,
    /// because desktop GLSL has no 1.00.
    pub fn from_directive(number: u16, es: bool) -> Option<Version> {
        if es || number == 100 {
            EsVersion::from_number(number).map(Version::Es)
        } else {
            DesktopVersion::from_number(number).map(Version::Desktop)
        }
    }

    /// The version an undeclared source is treated as: GLSL 1.10, per the
    /// spec's own rule that a missing `#version` means 110.
    pub const DEFAULT: Version = Version::Desktop(DesktopVersion::V110);

    pub const fn number(self) -> u16 {
        match self {
            Version::Desktop(v) => v.number(),
            Version::Es(v) => v.number(),
        }
    }

    pub const fn is_es(self) -> bool {
        matches!(self, Version::Es(_))
    }

    /// `4.50` or `3.00 es`.
    pub fn label(self) -> String {
        match self {
            Version::Desktop(v) => v.label().to_string(),
            Version::Es(v) => format!("{} es", v.label()),
        }
    }
}

/// Availability across both profiles — the pair every table entry carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Availability {
    pub desktop: DesktopMask,
    pub es: EsMask,
}

impl Availability {
    pub const NOWHERE: Availability =
        Availability { desktop: DesktopMask::EMPTY, es: EsMask::EMPTY };

    pub const fn new(desktop: DesktopMask, es: EsMask) -> Availability {
        Availability { desktop, es }
    }

    pub const fn contains(self, version: Version) -> bool {
        match version {
            Version::Desktop(v) => self.desktop.contains(v),
            Version::Es(v) => self.es.contains(v),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.desktop.is_empty() && self.es.is_empty()
    }

    pub const fn union(self, other: Availability) -> Availability {
        Availability {
            desktop: self.desktop.union(other.desktop),
            es: self.es.union(other.es),
        }
    }
}
