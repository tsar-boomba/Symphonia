// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::identity_op)]
#![allow(clippy::manual_range_contains)]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use symphonia_core::codecs::CodecInfo;
use symphonia_core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia_core::{async_trait, support_audio_codec};

use symphonia_core::audio::conv::IntoSample;
use symphonia_core::audio::sample::SampleFormat;
use symphonia_core::audio::{
    AsGenericAudioBufferRef, Audio, AudioMut, AudioSpec, GenericAudioBuffer, GenericAudioBufferRef,
};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_ALAW, CODEC_ID_PCM_MULAW};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64LE};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_S8, CODEC_ID_PCM_S16LE};
use symphonia_core::codecs::audio::well_known::{
    CODEC_ID_PCM_S16BE, CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S32BE,
};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32LE};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_U8, CODEC_ID_PCM_U16LE};
use symphonia_core::codecs::audio::well_known::{
    CODEC_ID_PCM_U16BE, CODEC_ID_PCM_U24BE, CODEC_ID_PCM_U32BE,
};
use symphonia_core::codecs::audio::well_known::{CODEC_ID_PCM_U24LE, CODEC_ID_PCM_U32LE};
use symphonia_core::codecs::audio::{
    AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia_core::errors::{Result, decode_error, unsupported_error};
use symphonia_core::io::ReadBytes;
use symphonia_core::packet::Packet;

macro_rules! decode_pcm_signed {
    ($buf:expr, $variant:tt, $reader:ident, $read_fn:ident, $shift:expr) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        let raw = $reader.$read_fn().await?;
                        // SAFETY: resize_uninit guarantees the index is valid.
                        buf.plane_mut(ch).unwrap()[frame] = (raw << ($shift as u32)).into_sample();
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! decode_pcm_signed_24 {
    ($buf:expr, $variant:tt, $reader:ident, $read_fn:ident, $coded_width:expr) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let extra_shift = 24u32 - $coded_width;
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        // read_i24 / read_be_i24 return the value in the low 24 bits of an i32.
                        // Shift left by 8 to occupy the full i32 range, then apply extra_shift.
                        let raw = $reader.$read_fn().await? << 8i32;
                        buf.plane_mut(ch).unwrap()[frame] = (raw << extra_shift).into_sample();
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! decode_pcm_unsigned {
    ($buf:expr, $variant:tt, $reader:ident, $read_fn:ident, $shift:expr) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        let raw = $reader.$read_fn().await?;
                        buf.plane_mut(ch).unwrap()[frame] = (raw << ($shift as u32)).into_sample();
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! decode_pcm_unsigned_24 {
    ($buf:expr, $variant:tt, $reader:ident, $read_fn:ident, $coded_width:expr) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let extra_shift = 24u32 - $coded_width;
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        let raw = $reader.$read_fn().await? << 8u32;
                        buf.plane_mut(ch).unwrap()[frame] = (raw << extra_shift).into_sample();
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! decode_pcm_floating {
    ($buf:expr, $variant:tt, $reader:ident, $read_fn:ident) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        buf.plane_mut(ch).unwrap()[frame] = $reader.$read_fn().await?;
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

macro_rules! decode_pcm_transfer {
    ($buf:expr, $variant:tt, $reader:ident, $transfer_fn:expr) => {{
        match $buf {
            GenericAudioBuffer::$variant(ref mut buf) => {
                let num_frames = buf.capacity();
                let num_channels = buf.spec().channels().count();
                buf.resize_uninit(num_frames);
                for frame in 0..num_frames {
                    for ch in 0..num_channels {
                        let raw = $reader.read_u8().await?;
                        buf.plane_mut(ch).unwrap()[frame] = $transfer_fn(raw);
                    }
                }
                Result::<()>::Ok(())
            }
            _ => unreachable!(),
        }
    }};
}

const XLAW_QUANT_MASK: u8 = 0x0f;
const XLAW_SEG_MASK: u8 = 0x70;
const XLAW_SEG_SHIFT: u32 = 4;

fn alaw_to_linear(mut a_val: u8) -> i16 {
    a_val ^= 0x55;
    let mut t = i16::from((a_val & XLAW_QUANT_MASK) << 4);
    let seg = (a_val & XLAW_SEG_MASK) >> XLAW_SEG_SHIFT;
    match seg {
        0 => t += 0x8,
        1 => t += 0x108,
        _ => t = (t + 0x108) << (seg - 1),
    }
    if a_val & 0x80 == 0x80 { t } else { -t }
}

fn mulaw_to_linear(mut mu_val: u8) -> i16 {
    const BIAS: i16 = 0x84;
    mu_val = !mu_val;
    let mut t = i16::from((mu_val & XLAW_QUANT_MASK) << 3) + BIAS;
    t <<= (mu_val & XLAW_SEG_MASK) >> XLAW_SEG_SHIFT;
    if mu_val & 0x80 == 0x80 { BIAS - t } else { t - BIAS }
}

fn is_supported_pcm_codec(codec_id: AudioCodecId) -> bool {
    matches!(
        codec_id,
        CODEC_ID_PCM_S32LE
            | CODEC_ID_PCM_S32BE
            | CODEC_ID_PCM_S24LE
            | CODEC_ID_PCM_S24BE
            | CODEC_ID_PCM_S16LE
            | CODEC_ID_PCM_S16BE
            | CODEC_ID_PCM_S8
            | CODEC_ID_PCM_U32LE
            | CODEC_ID_PCM_U32BE
            | CODEC_ID_PCM_U24LE
            | CODEC_ID_PCM_U24BE
            | CODEC_ID_PCM_U16LE
            | CODEC_ID_PCM_U16BE
            | CODEC_ID_PCM_U8
            | CODEC_ID_PCM_F32LE
            | CODEC_ID_PCM_F32BE
            | CODEC_ID_PCM_F64LE
            | CODEC_ID_PCM_F64BE
            | CODEC_ID_PCM_ALAW
            | CODEC_ID_PCM_MULAW
    )
}

// ---------------------------------------------------------------------------
// Decoder struct
// ---------------------------------------------------------------------------

/// Pulse Code Modulation (PCM) decoder for all raw PCM and log-PCM codecs.
pub struct PcmDecoder {
    params: AudioCodecParameters,
    coded_width: u32,
    buf: GenericAudioBuffer,
}

impl PcmDecoder {
    pub fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        if !is_supported_pcm_codec(params.codec) {
            return unsupported_error("pcm: invalid codec");
        }

        let frames = match params.max_frames_per_packet {
            Some(frames) => frames as usize,
            _ => return unsupported_error("pcm: maximum frames per packet is required"),
        };

        let rate = match params.sample_rate {
            Some(rate) => rate,
            _ => return unsupported_error("pcm: sample rate is required"),
        };

        let spec = if let Some(channels) = &params.channels {
            if channels.count() < 1 {
                return unsupported_error("pcm: number of channels cannot be 0");
            }
            AudioSpec::new(rate, channels.clone())
        }
        else {
            return unsupported_error("pcm: channels or channel_layout is required");
        };

        let (sample_format, sample_format_width) = match params.codec {
            CODEC_ID_PCM_S32LE | CODEC_ID_PCM_S32BE => (SampleFormat::S32, 32),
            CODEC_ID_PCM_S24LE | CODEC_ID_PCM_S24BE => (SampleFormat::S24, 24),
            CODEC_ID_PCM_S16LE | CODEC_ID_PCM_S16BE => (SampleFormat::S16, 16),
            CODEC_ID_PCM_S8 => (SampleFormat::S8, 8),
            CODEC_ID_PCM_U32LE | CODEC_ID_PCM_U32BE => (SampleFormat::U32, 32),
            CODEC_ID_PCM_U24LE | CODEC_ID_PCM_U24BE => (SampleFormat::U24, 24),
            CODEC_ID_PCM_U16LE | CODEC_ID_PCM_U16BE => (SampleFormat::U16, 16),
            CODEC_ID_PCM_U8 => (SampleFormat::U8, 8),
            CODEC_ID_PCM_F32LE | CODEC_ID_PCM_F32BE => (SampleFormat::F32, 32),
            CODEC_ID_PCM_F64LE | CODEC_ID_PCM_F64BE => (SampleFormat::F64, 64),
            CODEC_ID_PCM_ALAW | CODEC_ID_PCM_MULAW => (SampleFormat::S16, 16),
            _ => unreachable!(),
        };

        let coded_width =
            params.bits_per_coded_sample.unwrap_or_else(|| params.bits_per_sample.unwrap_or(0));

        if coded_width == 0 {
            match params.codec {
                CODEC_ID_PCM_F32LE | CODEC_ID_PCM_F32BE => (),
                CODEC_ID_PCM_F64LE | CODEC_ID_PCM_F64BE => (),
                CODEC_ID_PCM_ALAW | CODEC_ID_PCM_MULAW => (),
                _ => return unsupported_error("pcm: unknown bits per (coded) sample"),
            }
        }
        else if coded_width > sample_format_width {
            return decode_error("pcm: coded bits per sample is greater than the sample format");
        }

        let buf = GenericAudioBuffer::new(sample_format, spec, frames);

        Ok(PcmDecoder { params: params.clone(), coded_width, buf })
    }

    async fn decode_inner(&mut self, packet: &Packet) -> Result<()> {
        let mut reader = packet.as_buf_reader();

        // clear() resets num_frames to 0 while keeping the capacity allocation.
        self.buf.clear();

        let cw = self.coded_width; // borrow-checker friendliness

        match self.params.codec {
            CODEC_ID_PCM_S32LE => {
                decode_pcm_signed!(self.buf, S32, reader, read_i32, 32 - cw)?;
            }
            CODEC_ID_PCM_S24LE => {
                decode_pcm_signed_24!(self.buf, S24, reader, read_i24, cw)?;
            }
            CODEC_ID_PCM_S16LE => {
                decode_pcm_signed!(self.buf, S16, reader, read_i16, 16 - cw)?;
            }
            CODEC_ID_PCM_S8 => {
                decode_pcm_signed!(self.buf, S8, reader, read_i8, 8 - cw)?;
            }
            CODEC_ID_PCM_S32BE => {
                decode_pcm_signed!(self.buf, S32, reader, read_be_i32, 32 - cw)?;
            }
            CODEC_ID_PCM_S24BE => {
                decode_pcm_signed_24!(self.buf, S24, reader, read_be_i24, cw)?;
            }
            CODEC_ID_PCM_S16BE => {
                decode_pcm_signed!(self.buf, S16, reader, read_be_i16, 16 - cw)?;
            }
            CODEC_ID_PCM_U32LE => {
                decode_pcm_unsigned!(self.buf, U32, reader, read_u32, 32 - cw)?;
            }
            CODEC_ID_PCM_U24LE => {
                decode_pcm_unsigned_24!(self.buf, U24, reader, read_u24, cw)?;
            }
            CODEC_ID_PCM_U16LE => {
                decode_pcm_unsigned!(self.buf, U16, reader, read_u16, 16 - cw)?;
            }
            CODEC_ID_PCM_U8 => {
                decode_pcm_unsigned!(self.buf, U8, reader, read_u8, 8 - cw)?;
            }
            CODEC_ID_PCM_U32BE => {
                decode_pcm_unsigned!(self.buf, U32, reader, read_be_u32, 32 - cw)?;
            }
            CODEC_ID_PCM_U24BE => {
                decode_pcm_unsigned_24!(self.buf, U24, reader, read_be_u24, cw)?;
            }
            CODEC_ID_PCM_U16BE => {
                decode_pcm_unsigned!(self.buf, U16, reader, read_be_u16, 16 - cw)?;
            }
            CODEC_ID_PCM_F32LE => {
                decode_pcm_floating!(self.buf, F32, reader, read_f32)?;
            }
            CODEC_ID_PCM_F32BE => {
                decode_pcm_floating!(self.buf, F32, reader, read_be_f32)?;
            }
            CODEC_ID_PCM_F64LE => {
                decode_pcm_floating!(self.buf, F64, reader, read_f64)?;
            }
            CODEC_ID_PCM_F64BE => {
                decode_pcm_floating!(self.buf, F64, reader, read_be_f64)?;
            }
            CODEC_ID_PCM_ALAW => {
                decode_pcm_transfer!(self.buf, S16, reader, alaw_to_linear)?;
            }
            CODEC_ID_PCM_MULAW => {
                decode_pcm_transfer!(self.buf, S16, reader, mulaw_to_linear)?;
            }

            _ => return unsupported_error("pcm: codec is unsupported"),
        }

        Ok(())
    }
}

#[async_trait]
impl AudioDecoder for PcmDecoder {
    fn reset(&mut self) {
        // No inter-packet state; nothing to reset.
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs().iter().find(|desc| desc.id == self.params.codec).unwrap().info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    async fn decode(&mut self, packet: &Packet) -> Result<GenericAudioBufferRef<'_>> {
        if let Err(e) = self.decode_inner(packet).await {
            self.buf.clear();
            Err(e)
        }
        else {
            Ok(self.buf.as_generic_audio_buffer_ref())
        }
    }

    fn finalize(&mut self) -> FinalizeResult {
        Default::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buf.as_generic_audio_buffer_ref()
    }
}

#[async_trait]
impl RegisterableAudioDecoder for PcmDecoder {
    async fn try_registry_new(
        params: &AudioCodecParameters,
        opts: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(PcmDecoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[
            support_audio_codec!(
                CODEC_ID_PCM_S32LE,
                "pcm_s32le",
                "PCM Signed 32-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_S32BE,
                "pcm_s32be",
                "PCM Signed 32-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_S24LE,
                "pcm_s24le",
                "PCM Signed 24-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_S24BE,
                "pcm_s24be",
                "PCM Signed 24-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_S16LE,
                "pcm_s16le",
                "PCM Signed 16-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_S16BE,
                "pcm_s16be",
                "PCM Signed 16-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(CODEC_ID_PCM_S8, "pcm_s8", "PCM Signed 8-bit Interleaved"),
            support_audio_codec!(
                CODEC_ID_PCM_U32LE,
                "pcm_u32le",
                "PCM Unsigned 32-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_U32BE,
                "pcm_u32be",
                "PCM Unsigned 32-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_U24LE,
                "pcm_u24le",
                "PCM Unsigned 24-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_U24BE,
                "pcm_u24be",
                "PCM Unsigned 24-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_U16LE,
                "pcm_u16le",
                "PCM Unsigned 16-bit Little-Endian Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_U16BE,
                "pcm_u16be",
                "PCM Unsigned 16-bit Big-Endian Interleaved"
            ),
            support_audio_codec!(CODEC_ID_PCM_U8, "pcm_u8", "PCM Unsigned 8-bit Interleaved"),
            support_audio_codec!(
                CODEC_ID_PCM_F32LE,
                "pcm_f32le",
                "PCM 32-bit Little-Endian Floating Point Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_F32BE,
                "pcm_f32be",
                "PCM 32-bit Big-Endian Floating Point Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_F64LE,
                "pcm_f64le",
                "PCM 64-bit Little-Endian Floating Point Interleaved"
            ),
            support_audio_codec!(
                CODEC_ID_PCM_F64BE,
                "pcm_f64be",
                "PCM 64-bit Big-Endian Floating Point Interleaved"
            ),
            support_audio_codec!(CODEC_ID_PCM_ALAW, "pcm_alaw", "PCM A-law"),
            support_audio_codec!(CODEC_ID_PCM_MULAW, "pcm_mulaw", "PCM Mu-law"),
        ]
    }
}
