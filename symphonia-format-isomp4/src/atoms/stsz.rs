// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ops::Range;

use crate::atoms::{Atom, AtomHeader, AtomIterator, ReadAtom, Result, unsupported_error};
use alloc::vec::Vec;
use symphonia_core::errors::Error;

#[derive(Debug)]
pub enum VariableSampleSize {
    Thin(Vec<u16>),
    Fat(Vec<u32>),
}

impl VariableSampleSize {
    pub fn get(&self, i: usize) -> Option<u32> {
        match self {
            VariableSampleSize::Thin(items) => items.get(i).copied().map(u32::from),
            VariableSampleSize::Fat(items) => items.get(i).copied(),
        }
    }

    pub fn sum_range(&self, range: Range<usize>) -> Option<u64> {
        match self {
            Self::Thin(entries) => {
                entries.get(range).map(|i| i.iter().map(|&size| u64::from(size)).sum())
            }
            Self::Fat(entries) => {
                entries.get(range).map(|i| i.iter().map(|&size| u64::from(size)).sum())
            }
        }
    }

    /// Returns thin variant, which will grow to fat when necessary
    fn with_capacity(cap: usize) -> Self {
        Self::Thin(Vec::with_capacity(cap))
    }

    fn push(&mut self, count: u32) {
        match self {
            VariableSampleSize::Thin(items) => match u16::try_from(count) {
                Ok(count) => {
                    items.push(count);
                }
                Err(_) => {
                    // Must move to fat variant
                    let mut fat: Vec<u32> = Vec::with_capacity(items.capacity());

                    for i in items {
                        fat.push(*i as u32);
                    }
                    fat.push(count);

                    *self = Self::Fat(fat);
                }
            },
            VariableSampleSize::Fat(items) => items.push(count),
        }
    }
}

#[derive(Debug)]
pub enum SampleSize {
    Constant(u32),
    Variable(VariableSampleSize),
}

/// Sample Size Atom
#[allow(dead_code)]
#[derive(Debug)]
pub struct StszAtom {
    /// The total number of samples.
    pub sample_count: u32,
    /// A vector of `sample_count` sample sizes, or a constant size for all samples.
    pub sample_sizes: SampleSize,
}

impl Atom for StszAtom {
    async fn read<R: ReadAtom>(it: &mut AtomIterator<R>, header: &AtomHeader) -> Result<Self> {
        let (_, _) = it.read_extended_header().await?;

        let sample_size = it.read_u32().await?;
        let sample_count = it.read_u32().await?;

        let sample_sizes = if sample_size == 0 {
            if let Some(data_size) = header.data_size() {
                // Bound sample_count by using total atom size
                if u64::from(sample_count) * 4
                    > data_size
                        .checked_sub(12)
                        .ok_or(Error::Unsupported("isomp4: stsz header malformed"))?
                {
                    return unsupported_error("isomp4: sample count too large");
                }
            } else {
                return unsupported_error("isomp4: stsz has no header data");
            }

            let mut entries = VariableSampleSize::with_capacity(sample_count as usize);

            for _ in 0..sample_count {
                entries.push(it.read_u32().await?);
            }

            SampleSize::Variable(entries)
        } else {
            SampleSize::Constant(sample_size)
        };

        Ok(StszAtom { sample_count, sample_sizes })
    }
}
