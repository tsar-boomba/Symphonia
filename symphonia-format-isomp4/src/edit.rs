// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Interpretation of track edit lists.
//!
//! For audio, an edit list almost always holds a single edit that says which part of the media
//! is presented: playback starts at `media_time`, skipping the encoder's delay (priming) frames,
//! and lasts `segment_duration`, which cuts off the encoder's padding at the end. This is how
//! gapless playback is signalled for AAC, Opus, and other codecs in MP4.
//!
//! Only that form (optionally preceded by empty edits) is supported. Anything else, such as
//! multiple edits, dwells, or non-unity rates, is ignored and the whole media is presented.
//!
//! Apple's encoders (iTunes, Music, `afconvert`) don't write an edit list. They signal the same
//! thing with the `iTunSMPB` iTunes metadata tag instead, which is also interpreted here.

use core::num::NonZero;

use log::{debug, warn};

use crate::atoms::{ElstAtom, ElstEntry};

/// The presented range of a track's media timeline, in the track's (mdhd) timescale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackEdit {
    /// Media time of the first presented sample. Media before it is decoder delay.
    pub start: u64,
    /// Media time where presentation ends, if the edit has a duration. Media from here on is
    /// padding.
    pub end: Option<u64>,
}

impl TrackEdit {
    /// Interprets an edit list. Returns `None` if the edit list doesn't trim anything or isn't of
    /// a supported form, in which case the whole media should be presented.
    pub(crate) fn from_elst(
        elst: &ElstAtom,
        movie_timescale: NonZero<u32>,
        track_timescale: NonZero<u32>,
    ) -> Option<Self> {
        Self::from_entries(&elst.entries, movie_timescale, track_timescale)
    }

    fn from_entries(
        entries: &[ElstEntry],
        movie_timescale: NonZero<u32>,
        track_timescale: NonZero<u32>,
    ) -> Option<Self> {
        // Empty edits (media_time = -1) insert time before the media is presented. They do not
        // change which part of the media is presented, so they are skipped. A music player starts
        // playback immediately anyways.
        let mut edits = entries.iter().filter(|e| e.media_time != -1);

        let edit = edits.next()?;

        if edits.next().is_some() {
            warn!("isomp4: multiple edits are not supported, ignoring the edit list");
            return None;
        }

        if edit.media_rate_int != 1 || edit.media_rate_frac != 0 {
            warn!("isomp4: edits with a non-unity rate are not supported, ignoring the edit list");
            return None;
        }

        let Ok(start) = u64::try_from(edit.media_time)
        else {
            warn!("isomp4: invalid edit media time {}, ignoring the edit list", edit.media_time);
            return None;
        };

        // A zero duration means the edit lasts until the end of the media (common in fragmented
        // files, where the duration isn't known when the moov atom is written).
        let end = match edit.segment_duration {
            0 => None,
            dur => {
                let dur = rescale(dur, movie_timescale, track_timescale)?;
                Some(start.checked_add(dur)?)
            }
        };

        if start == 0 && end.is_none() {
            // Presents everything.
            return None;
        }

        debug!("isomp4: edit presents media time {start}..{end:?}");

        Some(TrackEdit { start, end })
    }
}

impl TrackEdit {
    /// Interprets the value of an `iTunSMPB` tag, which is a list of hex numbers separated by
    /// spaces, such as `" 00000000 00000840 00000270 00000000000D7550 00000000 ..."`. The second
    /// is the encoder delay, the third the padding, and the fourth the number of valid frames,
    /// all in the track's sample rate. Returns `None` if the value is malformed or doesn't trim
    /// anything.
    pub(crate) fn from_itunsmpb(value: &str) -> Option<Self> {
        let mut fields = value.split_ascii_whitespace().map(|f| u64::from_str_radix(f, 16));
        let _reserved = fields.next()?.ok()?;
        let delay = fields.next()?.ok()?;
        let _padding = fields.next()?.ok()?;
        let valid_frames = fields.next()?.ok()?;

        // The number of valid frames is sometimes left as 0, in which case only the delay is
        // known.
        let end = match valid_frames {
            0 => None,
            n => Some(delay.checked_add(n)?),
        };

        if delay == 0 && end.is_none() {
            return None;
        }

        debug!("isomp4: iTunSMPB presents media time {delay}..{end:?}");

        Some(TrackEdit { start: delay, end })
    }
}

/// Converts `value` from timescale `from` to timescale `to`, rounding to the nearest unit.
fn rescale(value: u64, from: NonZero<u32>, to: NonZero<u32>) -> Option<u64> {
    let from = u128::from(from.get());
    let scaled = (u128::from(value) * u128::from(to.get()) + from / 2) / from;
    u64::try_from(scaled).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(v: u32) -> NonZero<u32> {
        NonZero::new(v).unwrap()
    }

    fn edit(segment_duration: u64, media_time: i64) -> ElstEntry {
        ElstEntry { segment_duration, media_time, media_rate_int: 1, media_rate_frac: 0 }
    }

    #[test]
    fn single_edit_trims_start_and_end() {
        // From a real Opus .m4a (ffmpeg): movie timescale 1000, 147973 ms presented from 312.
        let e = TrackEdit::from_entries(&[edit(147_973, 312)], ts(1000), ts(48_000));
        assert_eq!(e, Some(TrackEdit { start: 312, end: Some(312 + 147_973 * 48) }));
    }

    #[test]
    fn same_timescale_is_exact() {
        // iTunes-style AAC: 2112 frames of priming.
        let e = TrackEdit::from_entries(&[edit(441_000, 2112)], ts(44_100), ts(44_100));
        assert_eq!(e, Some(TrackEdit { start: 2112, end: Some(2112 + 441_000) }));
    }

    #[test]
    fn rescale_rounds_to_nearest() {
        // 1 ms at 44.1 kHz is 44.1 frames.
        assert_eq!(rescale(1, ts(1000), ts(44_100)), Some(44));
        // 3 ms is 132.3 frames, 7 ms is 308.7 frames.
        assert_eq!(rescale(3, ts(1000), ts(44_100)), Some(132));
        assert_eq!(rescale(7, ts(1000), ts(44_100)), Some(309));
    }

    #[test]
    fn leading_empty_edit_is_skipped() {
        let e = TrackEdit::from_entries(&[edit(500, -1), edit(1000, 1024)], ts(1000), ts(1000));
        assert_eq!(e, Some(TrackEdit { start: 1024, end: Some(2024) }));
    }

    #[test]
    fn zero_duration_means_until_the_end() {
        let e = TrackEdit::from_entries(&[edit(0, 1024)], ts(1000), ts(44_100));
        assert_eq!(e, Some(TrackEdit { start: 1024, end: None }));
    }

    #[test]
    fn no_trim_is_none() {
        assert_eq!(TrackEdit::from_entries(&[], ts(1000), ts(1000)), None);
        assert_eq!(TrackEdit::from_entries(&[edit(0, 0)], ts(1000), ts(1000)), None);
        assert_eq!(TrackEdit::from_entries(&[edit(10, -1)], ts(1000), ts(1000)), None);
    }

    #[test]
    fn itunsmpb() {
        // From an `afconvert` AAC file: 2112 frames of delay, 624 of padding, 882000 valid.
        let smpb = " 00000000 00000840 00000270 00000000000D7550 00000000 00000000 00000000 \
                    00000000 00000000 00000000 00000000 00000000";
        assert_eq!(
            TrackEdit::from_itunsmpb(smpb),
            Some(TrackEdit { start: 2112, end: Some(2112 + 882_000) })
        );

        // Unknown length: only the delay
        assert_eq!(
            TrackEdit::from_itunsmpb(" 00000000 00000840 00000000 0000000000000000"),
            Some(TrackEdit { start: 2112, end: None })
        );

        // Nothing to trim, or malformed
        assert_eq!(TrackEdit::from_itunsmpb(" 00000000 00000000 00000000 0000000000000000"), None);
        assert_eq!(TrackEdit::from_itunsmpb(" 00000000 00000840 00000270"), None);
        assert_eq!(TrackEdit::from_itunsmpb(" 00000000 zz 00000270 0000000000000010"), None);
        assert_eq!(TrackEdit::from_itunsmpb(""), None);
    }

    #[test]
    fn unsupported_forms_are_ignored() {
        // Multiple edits
        let two = [edit(100, 0), edit(100, 500)];
        assert_eq!(TrackEdit::from_entries(&two, ts(1000), ts(1000)), None);

        // Dwell (rate 0) and fast-forward
        let dwell = ElstEntry { media_rate_int: 0, ..edit(100, 10) };
        assert_eq!(TrackEdit::from_entries(&[dwell], ts(1000), ts(1000)), None);
        let half = ElstEntry { media_rate_frac: 0x8000u16 as i16, ..edit(100, 10) };
        assert_eq!(TrackEdit::from_entries(&[half], ts(1000), ts(1000)), None);

        // Negative media time other than the empty edit marker
        assert_eq!(TrackEdit::from_entries(&[edit(100, -5)], ts(1000), ts(1000)), None);
    }
}
