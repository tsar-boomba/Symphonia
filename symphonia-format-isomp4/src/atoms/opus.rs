// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use alloc::boxed::Box;
use symphonia_common::xiph::audio::opus;
use symphonia_core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia_core::errors::Error;
use symphonia_core::io::BufReader;

use crate::atoms::stsd::AudioSampleEntry;
use crate::atoms::{
    Atom, AtomHeader, AtomIterator, ReadAtom, Result, decode_error, unsupported_error,
};

const OPUS_MAGIC: &[u8] = b"OpusHead";
const OPUS_MAGIC_LEN: usize = OPUS_MAGIC.len();

/// Opus atom.
#[allow(dead_code)]
#[derive(Debug)]
pub struct OpusAtom {
    /// Opus extra data (identification header).
    extra_data: Box<[u8]>,
}

impl Atom for OpusAtom {
    async fn read<R: ReadAtom>(reader: &mut AtomIterator<R>, header: &AtomHeader) -> Result<Self> {
        const MIN_OPUS_EXTRA_DATA_SIZE: usize = OPUS_MAGIC_LEN + 11;
        const MAX_OPUS_EXTRA_DATA_SIZE: usize = MIN_OPUS_EXTRA_DATA_SIZE + 257;

        // Offset of the Opus version number in the extra data.
        const OPUS_EXTRADATA_VERSION_OFFSET: usize = OPUS_MAGIC_LEN;

        // The dops atom contains an Opus identification header excluding the OpusHead magic
        // signature. Therefore, the atom data length should be atleast as long as the shortest
        // Opus identification header.
        let data_len = header
            .data_size()
            .ok_or(Error::DecodeError("isomp4 (opus): expected atom size to be known"))?
            as usize;

        if data_len < MIN_OPUS_EXTRA_DATA_SIZE - OPUS_MAGIC_LEN {
            return decode_error("isomp4 (opus): opus identification header too short");
        }

        if data_len > MAX_OPUS_EXTRA_DATA_SIZE - OPUS_MAGIC_LEN {
            return decode_error("isomp4 (opus): opus identification header too large");
        }

        let mut extra_data = vec![0; OPUS_MAGIC_LEN + data_len].into_boxed_slice();

        // The Opus magic is excluded in the atom, but the extra data must start with it.
        extra_data[..OPUS_MAGIC_LEN].copy_from_slice(OPUS_MAGIC);

        // Read the extra data from the atom.
        reader.read_buf_exact(&mut extra_data[OPUS_MAGIC_LEN..]).await?;

        // Verify the version number is 0.
        if extra_data[OPUS_EXTRADATA_VERSION_OFFSET] != 0 {
            return unsupported_error("isomp4 (opus): unsupported opus version");
        }

        dops_to_opus_head(&mut extra_data);

        Ok(OpusAtom { extra_data })
    }
}

/// Rewrites `OpusHead` magic + `dOps` atom data in place into an Ogg Opus identification header
/// (RFC 7845, section 5.1), which is what decoders and [`opus::OpusHead::read`] expect.
///
/// The two carry the same fields, but `dOps` stores its multi-byte fields big-endian and its
/// version is 0, while the Ogg header is little-endian with version 1. The channel mapping table
/// after the fixed fields is single bytes in both, so it is left as is.
///
/// `extra_data` must be at least 19 bytes (magic + the fixed `dOps` fields).
fn dops_to_opus_head(extra_data: &mut [u8]) {
    const VERSION: usize = OPUS_MAGIC_LEN;
    const PRE_SKIP: usize = OPUS_MAGIC_LEN + 2;
    const SAMPLE_RATE: usize = OPUS_MAGIC_LEN + 4;
    const GAIN: usize = OPUS_MAGIC_LEN + 8;

    extra_data[VERSION] = 1;
    extra_data[PRE_SKIP..PRE_SKIP + 2].reverse();
    extra_data[SAMPLE_RATE..SAMPLE_RATE + 4].reverse();
    extra_data[GAIN..GAIN + 2].reverse();
}

impl OpusAtom {
    pub fn fill_audio_sample_entry(self, entry: &mut AudioSampleEntry) {
        let mut reader = BufReader::new(&self.extra_data);

        if let Ok(opus_head) = opus::OpusHead::read(&mut reader, 1) {
            entry.channels = Some(opus_head.channels);
            entry.sample_rate = 48_000.0;
        }

        entry.codec_id = CODEC_ID_OPUS;
        entry.extra_data = Some(self.extra_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dops_converts_to_ogg_opus_head() {
        // From a real file: dOps version 0, 2 channels, pre-skip 312, 48 kHz, gain -256 (-1 dB),
        // mapping family 0. Big-endian fields.
        let mut extra = *b"OpusHead\x00\x02\x01\x38\x00\x00\xbb\x80\xff\x00\x00";
        dops_to_opus_head(&mut extra);

        // What an Ogg Opus file would carry: version 1, little-endian fields.
        assert_eq!(&extra, b"OpusHead\x01\x02\x38\x01\x80\xbb\x00\x00\x00\xff\x00");

        let head = opus::OpusHead::read(&mut BufReader::new(&extra), 1).unwrap();
        assert_eq!(head.pre_skip, 312);
        assert_eq!(head.original_sample_rate, 48_000);
        assert_eq!(head.gain, -256);
        assert_eq!(head.channels.count(), 2);
    }
}
