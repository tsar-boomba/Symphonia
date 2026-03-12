// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use alloc::collections::{BTreeMap, BTreeSet};

use symphonia_core::errors::Result;
use symphonia_core::io::{
    MediaSourceStream, ReadBytes, ScopedStream, Seek, SeekBuffered, SeekFrom,
};

use super::logical::{InspectState, LogicalStream};
use super::page::*;

use log::debug;

pub async fn probe_stream_start(
    reader: &mut MediaSourceStream<'_>,
    pages: &mut PageReader,
    streams: &mut BTreeMap<u32, LogicalStream>,
) {
    // Save the original position to jump back to.
    let original_pos = reader.pos();

    // Scope the reader the prevent overruning the seekback region.
    let mut scoped_reader = ScopedStream::new(reader, OGG_PAGE_MAX_SIZE as u64);

    let mut probed = BTreeSet::<u32>::new();

    // Examine the first bitstream page of each logical stream within the physical stream to
    // determine the number of leading samples, and start time. This function is called assuming
    // the page reader is on the first bitstream page within the physical stream.
    loop {
        let page = pages.page();

        // If the page does not belong to the current physical stream, break out.
        let stream = if let Some(stream) = streams.get_mut(&page.header.serial) {
            stream
        }
        else {
            break;
        };

        // If the stream hasn't been marked as probed.
        if !probed.contains(&page.header.serial) {
            // Probe the first page of the logical stream.
            stream.inspect_start_page(&page).await;
            // Mark the logical stream as probed.
            probed.insert(page.header.serial);
        }

        // If all logical streams were probed, break out immediately.
        if probed.len() >= streams.len() {
            break;
        }

        // Read the next page.
        match pages.try_next_page(&mut scoped_reader).await {
            Ok(_) => (),
            _ => break,
        };
    }

    scoped_reader.into_inner().seek_buffered(original_pos);
}

pub async fn probe_stream_end(
    reader: &mut MediaSourceStream<'_>,
    pages: &mut PageReader,
    streams: &mut BTreeMap<u32, LogicalStream>,
    byte_range_start: u64,
    byte_range_end: u64,
) -> Result<Option<u64>> {
    // Save the original position.
    let original_pos = reader.pos();

    // Number of bytes to linearly scan. We assume the OGG maximum page size for each logical
    // stream.
    let linear_scan_len = (streams.len() * OGG_PAGE_MAX_SIZE) as u64;

    // Optimization: Try a linear scan of the last few pages first. This will cover all
    // non-chained physical streams, which is the majority of cases.
    if byte_range_end >= linear_scan_len && byte_range_start <= byte_range_end - linear_scan_len {
        reader.seek(SeekFrom::Start(byte_range_end - linear_scan_len)).await?;
    }
    else {
        reader.seek(SeekFrom::Start(byte_range_start)).await?;
    }

    pages.next_page(reader).await?;

    let result = scan_stream_end(reader, pages, streams, byte_range_end).await;

    // If there are no pages belonging to the current physical stream at the end of the media
    // source stream, then one or more physical streams are chained. Use a bisection method to find
    // the end of the current physical stream.
    let result = if result.is_none() {
        debug!("media source stream is chained, bisecting end of physical stream");

        let mut start = byte_range_start;
        let mut end = byte_range_end;

        loop {
            let mid = (end + start) / 2;
            reader.seek(SeekFrom::Start(mid)).await?;

            match pages.next_page(reader).await {
                Ok(_) => (),
                _ => break,
            }

            let header = pages.header();

            if streams.contains_key(&header.serial) {
                start = mid;
            }
            else {
                end = mid;
            }

            if end - start < linear_scan_len {
                break;
            }
        }

        // Scan the last few pages of the physical stream.
        reader.seek(SeekFrom::Start(start)).await?;

        pages.next_page(reader).await?;

        scan_stream_end(reader, pages, streams, end).await
    }
    else {
        result
    };

    // Restore the original position
    reader.seek(SeekFrom::Start(original_pos)).await?;

    Ok(result)
}

async fn scan_stream_end(
    reader: &mut MediaSourceStream<'_>,
    pages: &mut PageReader,
    streams: &mut BTreeMap<u32, LogicalStream>,
    byte_range_end: u64,
) -> Option<u64> {
    let scoped_len = byte_range_end - reader.pos();

    let mut scoped_reader = ScopedStream::new(reader, scoped_len);

    let mut upper_pos = None;

    let mut state = InspectState::default();

    // Read pages until the provided end position or a new physical stream starts.
    loop {
        let page = pages.page();

        // If the page does not belong to the current physical stream, then break out, the
        // extent of the physical stream has been found.
        let stream = if let Some(stream) = streams.get_mut(&page.header.serial) {
            stream
        }
        else {
            break;
        };

        state = stream.inspect_end_page(state, &page).await;

        // The new end of the physical stream is the position after this page.
        upper_pos = Some(scoped_reader.pos());

        // Read to the next page.
        match pages.next_page(&mut scoped_reader).await {
            Ok(_) => (),
            _ => break,
        }
    }

    upper_pos
}

pub async fn probe_stream_end_fast(
    reader: &mut MediaSourceStream<'_>,
    streams: &mut BTreeMap<u32, LogicalStream>,
    byte_range_end: u64,
) -> Result<Option<u64>> {
    let original_pos = reader.pos();
    
    // A page header is at most ~27 + 255 bytes. Search the last few KB only.
    let search_start = byte_range_end.saturating_sub(OGG_PAGE_MAX_SIZE as u64);
    
    reader.seek(SeekFrom::Start(search_start)).await?;
    
    // Read the search window into a buffer.
    let buf_len = (byte_range_end - search_start) as usize;
    let mut buf = vec![0u8; buf_len];
    reader.read_buf_exact(&mut buf).await?;
    
    // Scan backwards for the last OggS magic belonging to our streams.
    let magic = b"OggS";
    let mut last_granule = None;
    
    // Walk backwards through all OggS occurrences
    for i in (0..buf.len().saturating_sub(27)).rev() {
        if &buf[i..i+4] != magic {
            continue;
        }
        
        // Granule position is at bytes 6-13 of the page header (little-endian u64)
        let granule = u64::from_le_bytes(buf[i+6..i+14].try_into().unwrap());
        // Serial number is at bytes 14-17
        let serial = u32::from_le_bytes(buf[i+14..i+18].try_into().unwrap());
        
        if streams.contains_key(&serial) && granule != u64::MAX {
            last_granule = Some((serial, granule));
            break; // Found the last valid page for our stream
        }
    }
    
    // Restore position
    reader.seek(SeekFrom::Start(original_pos)).await?;
    
    // Update stream duration from granule position
    if let Some((serial, granule)) = last_granule
        && let Some(stream) = streams.get_mut(&serial) {
            stream.set_end_granule(granule);
        }
    
    Ok(last_granule.map(|_| byte_range_end))
}
