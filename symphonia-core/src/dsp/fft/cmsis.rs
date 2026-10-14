// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complex (I)FFT using the hardware-accelerated CMSIS-DSP implementation, where supported.

use cmsis_dsp::transform::{Direction, FloatFft, OutputOrder};

use crate::dsp::complex::Complex32;

use super::no_simd;

/// The minimum FFT size supported by the CMSIS-DSP complex FFT.
const MIN_HW_SIZE: usize = 16;
/// The maximum FFT size supported by the CMSIS-DSP complex FFT.
const MAX_HW_SIZE: usize = 4096;

/// Returns `true` if the CMSIS-DSP complex FFT supports a power-of-two size `n`.
fn is_hw_size(n: usize) -> bool {
    (MIN_HW_SIZE..=MAX_HW_SIZE).contains(&n)
}

/// Run the CMSIS-DSP complex FFT in-place.
fn hw_fft(x: &mut [Complex32], dir: Direction) {
    // The size is validated when the (I)FFT is instantiated.
    FloatFft::new(x.len() as u16).expect("supported cmsis-dsp fft size").run(
        x,
        dir,
        OutputOrder::Standard,
    );
}

enum FftImpl<T> {
    Hardware(usize),
    Software(T),
}

/// Complex forward FFT. Hardware-accelerated for power-of-two sizes from 16 to 4096, inclusive.
/// Other sizes fallback to the software implementation.
pub struct Fft {
    inner: FftImpl<no_simd::Fft>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "FFT size must be a power of two");

        let inner = if is_hw_size(n) {
            FftImpl::Hardware(n)
        }
        else {
            FftImpl::Software(no_simd::Fft::new(n))
        };

        Self { inner }
    }

    pub fn size(&self) -> usize {
        match &self.inner {
            FftImpl::Hardware(n) => *n,
            FftImpl::Software(fft) => fft.size(),
        }
    }

    /// In-place forward FFT.
    pub fn fft_inplace(&mut self, x: &mut [Complex32]) {
        match &mut self.inner {
            FftImpl::Hardware(n) => {
                assert_eq!(x.len(), *n);
                hw_fft(x, Direction::Forward);
            }
            FftImpl::Software(fft) => fft.fft_inplace(x),
        }
    }

    /// Out-of-place forward FFT.
    pub fn fft(&mut self, x: &[Complex32], y: &mut [Complex32]) {
        match &mut self.inner {
            FftImpl::Hardware(n) => {
                assert_eq!(x.len(), *n);
                assert_eq!(y.len(), *n);
                y.copy_from_slice(x);
                hw_fft(y, Direction::Forward);
            }
            FftImpl::Software(fft) => fft.fft(x, y),
        }
    }
}

/// Complex inverse FFT. Hardware-accelerated for power-of-two sizes from 16 to 4096, inclusive.
/// Other sizes fallback to the software implementation.
pub struct Ifft {
    inner: FftImpl<no_simd::Ifft>,
}

impl Ifft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "IFFT size must be a power of two");

        let inner = if is_hw_size(n) {
            FftImpl::Hardware(n)
        }
        else {
            FftImpl::Software(no_simd::Ifft::new(n))
        };

        Self { inner }
    }

    pub fn size(&self) -> usize {
        match &self.inner {
            FftImpl::Hardware(n) => *n,
            FftImpl::Software(ifft) => ifft.size(),
        }
    }

    /// In-place inverse FFT.
    pub fn ifft_inplace(&mut self, x: &mut [Complex32]) {
        match &mut self.inner {
            FftImpl::Hardware(n) => {
                assert_eq!(x.len(), *n);
                hw_fft(x, Direction::Inverse);
            }
            FftImpl::Software(ifft) => ifft.ifft_inplace(x),
        }
    }

    /// Out-of-place inverse FFT.
    pub fn ifft(&mut self, x: &[Complex32], y: &mut [Complex32]) {
        match &mut self.inner {
            FftImpl::Hardware(n) => {
                assert_eq!(x.len(), *n);
                assert_eq!(y.len(), *n);
                y.copy_from_slice(x);
                hw_fft(y, Direction::Inverse);
            }
            FftImpl::Software(ifft) => ifft.ifft(x, y),
        }
    }
}
