// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use symphonia_core::io::{ReadBitsLtr, vlc::*};

mod tables;

/// A codebook whose entry table is a pre-generated `static` table.
type StaticCodebook<E> = Codebook<E, &'static [Entry<E>]>;

static AAC_QUADS: [(u8, u8, u8, u8); 81] = [
    (0, 0, 0, 0),
    (0, 0, 0, 1),
    (0, 0, 0, 2),
    (0, 0, 1, 0),
    (0, 0, 1, 1),
    (0, 0, 1, 2),
    (0, 0, 2, 0),
    (0, 0, 2, 1),
    (0, 0, 2, 2),
    (0, 1, 0, 0),
    (0, 1, 0, 1),
    (0, 1, 0, 2),
    (0, 1, 1, 0),
    (0, 1, 1, 1),
    (0, 1, 1, 2),
    (0, 1, 2, 0),
    (0, 1, 2, 1),
    (0, 1, 2, 2),
    (0, 2, 0, 0),
    (0, 2, 0, 1),
    (0, 2, 0, 2),
    (0, 2, 1, 0),
    (0, 2, 1, 1),
    (0, 2, 1, 2),
    (0, 2, 2, 0),
    (0, 2, 2, 1),
    (0, 2, 2, 2),
    (1, 0, 0, 0),
    (1, 0, 0, 1),
    (1, 0, 0, 2),
    (1, 0, 1, 0),
    (1, 0, 1, 1),
    (1, 0, 1, 2),
    (1, 0, 2, 0),
    (1, 0, 2, 1),
    (1, 0, 2, 2),
    (1, 1, 0, 0),
    (1, 1, 0, 1),
    (1, 1, 0, 2),
    (1, 1, 1, 0),
    (1, 1, 1, 1),
    (1, 1, 1, 2),
    (1, 1, 2, 0),
    (1, 1, 2, 1),
    (1, 1, 2, 2),
    (1, 2, 0, 0),
    (1, 2, 0, 1),
    (1, 2, 0, 2),
    (1, 2, 1, 0),
    (1, 2, 1, 1),
    (1, 2, 1, 2),
    (1, 2, 2, 0),
    (1, 2, 2, 1),
    (1, 2, 2, 2),
    (2, 0, 0, 0),
    (2, 0, 0, 1),
    (2, 0, 0, 2),
    (2, 0, 1, 0),
    (2, 0, 1, 1),
    (2, 0, 1, 2),
    (2, 0, 2, 0),
    (2, 0, 2, 1),
    (2, 0, 2, 2),
    (2, 1, 0, 0),
    (2, 1, 0, 1),
    (2, 1, 0, 2),
    (2, 1, 1, 0),
    (2, 1, 1, 1),
    (2, 1, 1, 2),
    (2, 1, 2, 0),
    (2, 1, 2, 1),
    (2, 1, 2, 2),
    (2, 2, 0, 0),
    (2, 2, 0, 1),
    (2, 2, 0, 2),
    (2, 2, 1, 0),
    (2, 2, 1, 1),
    (2, 2, 1, 2),
    (2, 2, 2, 0),
    (2, 2, 2, 1),
    (2, 2, 2, 2),
];

/// Codebook for spectral quads.
pub struct QuadsCodebook {
    codebook: StaticCodebook<Entry16x16>,
}

impl QuadsCodebook {
    #[inline(always)]
    pub fn read_quant<B: ReadBitsLtr>(
        &self,
        bs: &mut B,
    ) -> symphonia_core::io::Result<(u8, u8, u8, u8)> {
        bs.read_codebook(&self.codebook).map(|(cw, _)| AAC_QUADS[cw as usize])
    }
}

/// Codebook for spectral pairs.
pub struct PairsCodebook {
    codebook: StaticCodebook<Entry16x16>,
    values: &'static [(f32, f32)],
}

impl PairsCodebook {
    #[inline(always)]
    pub fn read_dequant<B: ReadBitsLtr>(
        &self,
        bs: &mut B,
    ) -> symphonia_core::io::Result<(f32, f32)> {
        bs.read_codebook(&self.codebook).map(|(cw, _)| self.values[cw as usize])
    }
}

/// Codebook for spectral pairs with an escape code.
pub struct EscapeCodebook {
    codebook: StaticCodebook<Entry16x16>,
    values: &'static [(u16, u16)],
}

impl EscapeCodebook {
    #[inline(always)]
    pub fn read_quant<B: ReadBitsLtr>(&self, bs: &mut B) -> symphonia_core::io::Result<(u16, u16)> {
        bs.read_codebook(&self.codebook).map(|(cw, _)| self.values[cw as usize])
    }
}

pub static QUADS: [QuadsCodebook; 4] = [
    QuadsCodebook { codebook: tables::SPECTRUM1_CODEBOOK },
    QuadsCodebook { codebook: tables::SPECTRUM2_CODEBOOK },
    QuadsCodebook { codebook: tables::SPECTRUM3_CODEBOOK },
    QuadsCodebook { codebook: tables::SPECTRUM4_CODEBOOK },
];

pub static PAIRS: [PairsCodebook; 6] = [
    PairsCodebook { codebook: tables::SPECTRUM5_CODEBOOK, values: &tables::SPECTRUM5_VALUES },
    PairsCodebook { codebook: tables::SPECTRUM6_CODEBOOK, values: &tables::SPECTRUM6_VALUES },
    PairsCodebook { codebook: tables::SPECTRUM7_CODEBOOK, values: &tables::SPECTRUM7_VALUES },
    PairsCodebook { codebook: tables::SPECTRUM8_CODEBOOK, values: &tables::SPECTRUM8_VALUES },
    PairsCodebook { codebook: tables::SPECTRUM9_CODEBOOK, values: &tables::SPECTRUM9_VALUES },
    PairsCodebook { codebook: tables::SPECTRUM10_CODEBOOK, values: &tables::SPECTRUM10_VALUES },
];

pub static ESC: EscapeCodebook =
    EscapeCodebook { codebook: tables::SPECTRUM11_CODEBOOK, values: &tables::SPECTRUM11_VALUES };

pub static SCALEFACTORS: StaticCodebook<Entry8x16> = tables::SCF_CODEBOOK;
