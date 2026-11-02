// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// Previous Author: Kostya Shishkov <kostya.shiskov@gmail.com>
//
// This source file includes code originally written for the NihAV
// project. With the author's permission, it has been relicensed for,
// and ported to the Symphonia project.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use alloc::boxed::Box;
use alloc::vec;

use symphonia_core::dsp::complex::Complex;
use symphonia_core::dsp::mdct::Imdct;

use crate::aac::common::*;
use crate::aac::tables::{IMDCT_LONG_TWIDDLE, IMDCT_SHORT_TWIDDLE};
use crate::aac::tables::{KBD_LONG_WINDOW, KBD_SHORT_WINDOW, SINE_LONG_WINDOW, SINE_SHORT_WINDOW};

const SHORT_WIN_POINT0: usize = 512 - 64;
const SHORT_WIN_POINT1: usize = 512 + 64;

pub struct Dsp {
    imdct_long: Imdct<&'static [Complex<f32>]>,
    imdct_short: Imdct<&'static [Complex<f32>]>,
    // Boxed so that `Dsp` (and the decoder) is small enough to construct and move on the stack.
    pcm_long: Box<[f32; 2048]>,
    pcm_short: Box<[f32; 1152]>,
}

/// Allocate a zeroed array directly on the heap, without a temporary on the stack.
fn zeroed_box<const N: usize>() -> Box<[f32; N]> {
    vec![0.0; N].into_boxed_slice().try_into().expect("boxed slice of length N")
}

impl Dsp {
    pub fn new() -> Self {
        Self {
            imdct_long: Imdct::from_twiddle(&IMDCT_LONG_TWIDDLE),
            imdct_short: Imdct::from_twiddle(&IMDCT_SHORT_TWIDDLE),
            pcm_long: zeroed_box(),
            pcm_short: zeroed_box(),
        }
    }

    #[allow(clippy::cognitive_complexity)]
    pub fn synth(
        &mut self,
        coeffs: &[f32; 1024],
        delay: &mut [f32; 1024],
        seq: u8,
        window_shape: bool,
        prev_window_shape: bool,
        dst: &mut [f32],
    ) {
        let (long_win, short_win) = match window_shape {
            true => (&KBD_LONG_WINDOW, &KBD_SHORT_WINDOW),
            false => (&SINE_LONG_WINDOW, &SINE_SHORT_WINDOW),
        };

        let (prev_long_win, prev_short_win) = match prev_window_shape {
            true => (&KBD_LONG_WINDOW, &KBD_SHORT_WINDOW),
            false => (&SINE_LONG_WINDOW, &SINE_SHORT_WINDOW),
        };

        // Inverse MDCT
        if seq != EIGHT_SHORT_SEQUENCE {
            self.imdct_long.imdct(coeffs, &mut self.pcm_long[..]);
        }
        else {
            for (ain, aout) in coeffs.chunks_exact(128).zip(self.pcm_long.chunks_exact_mut(256)) {
                self.imdct_short.imdct(ain, aout);
            }

            // Zero the eight short sequence buffer.
            self.pcm_short.fill(0.0);

            for (w, src) in self.pcm_long.chunks_exact(256).enumerate() {
                if w > 0 {
                    for i in 0..128 {
                        self.pcm_short[w * 128 + i] += src[i] * short_win[i];
                        self.pcm_short[w * 128 + i + 128] += src[i + 128] * short_win[127 - i];
                    }
                }
                else {
                    for i in 0..128 {
                        self.pcm_short[i] = src[i] * prev_short_win[i];
                        self.pcm_short[i + 128] = src[i + 128] * short_win[127 - i];
                    }
                }
            }
        }

        // Output new audio samples.
        match seq {
            ONLY_LONG_SEQUENCE | LONG_START_SEQUENCE => {
                for i in 0..1024 {
                    dst[i] = delay[i] + (self.pcm_long[i] * prev_long_win[i]);
                }
            }
            EIGHT_SHORT_SEQUENCE => {
                dst[..SHORT_WIN_POINT0].copy_from_slice(&delay[..SHORT_WIN_POINT0]);

                for i in SHORT_WIN_POINT0..1024 {
                    dst[i] = delay[i] + self.pcm_short[i - SHORT_WIN_POINT0];
                }
            }
            LONG_STOP_SEQUENCE => {
                dst[..SHORT_WIN_POINT0].copy_from_slice(&delay[..SHORT_WIN_POINT0]);

                for i in SHORT_WIN_POINT0..SHORT_WIN_POINT1 {
                    dst[i] = delay[i] + self.pcm_long[i] * prev_short_win[i - SHORT_WIN_POINT0];
                }
                for i in SHORT_WIN_POINT1..1024 {
                    dst[i] = delay[i] + self.pcm_long[i];
                }
            }
            _ => unreachable!(),
        };

        // Save delay for overlap.
        match seq {
            ONLY_LONG_SEQUENCE | LONG_STOP_SEQUENCE => {
                for i in 0..1024 {
                    delay[i] = self.pcm_long[i + 1024] * long_win[1023 - i];
                }
            }
            EIGHT_SHORT_SEQUENCE => {
                for i in 0..SHORT_WIN_POINT1 {
                    // Last part is already windowed.
                    delay[i] = self.pcm_short[i + 512 + 64];
                }

                delay[SHORT_WIN_POINT1..].fill(0.0);
            }
            LONG_START_SEQUENCE => {
                delay[..SHORT_WIN_POINT0]
                    .copy_from_slice(&self.pcm_long[1024..(SHORT_WIN_POINT0 + 1024)]);

                for i in SHORT_WIN_POINT0..SHORT_WIN_POINT1 {
                    delay[i] = self.pcm_long[i + 1024] * short_win[127 - (i - SHORT_WIN_POINT0)];
                }

                delay[SHORT_WIN_POINT1..].fill(0.0);
            }
            _ => unreachable!(),
        };
    }
}
