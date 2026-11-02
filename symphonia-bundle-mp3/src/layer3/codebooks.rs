// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use symphonia_core::io::vlc::{Codebook, Entry};

mod tables;

/// A codebook whose entry table is a pre-generated `static` table.
type StaticCodebook<E> = Codebook<E, &'static [Entry<E>]>;

pub const CODEBOOK_LINBITS: [u32; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 6, 8, 10, 13, 4, 5, 6, 7, 8, 9, 11,
    13,
];

pub use tables::{CODEBOOK_TABLES, QUADS_CODEBOOK_TABLE};
