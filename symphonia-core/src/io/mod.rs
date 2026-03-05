// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `io` module implements composable bit- and byte-level I/O.
//!
//! The following nomenclature is used to denote where the data being read is sourced from:
//!  * A `Stream` consumes any source implementing [`ReadBytes`] one byte at a time.
//!  * A `Reader` consumes a `&[u8]`.
//!
//! The sole exception to this rule is [`MediaSourceStream`] which consumes sources implementing
//! [`MediaSource`] (aka. [`std::io::Read`]).
//!
//! All `Reader`s and `Stream`s operating on bytes of data at a time implement the [`ReadBytes`]
//! trait. Likewise, all `Reader`s and `Stream`s operating on bits of data at a time implement
//! either the [`ReadBitsLtr`] or [`ReadBitsRtl`] traits depending on the order in which they
//! consume bits.

use alloc::{borrow::Cow, boxed::Box, vec, vec::Vec};
use async_trait::async_trait;
use core::fmt::Debug;
use embedded_io as io;
use utils::{default_read_to_end, default_read_vectored};

mod bit;
mod buf_reader;
mod impls;
mod media_source_stream;
mod monitor_stream;
mod scoped_stream;
pub mod utils;

pub use bit::*;
pub use buf_reader::BufReader;
pub use embedded_io::{ErrorKind, ErrorType, ReadExactError, SeekFrom};
pub use media_source_stream::{MediaSourceStream, MediaSourceStreamOptions};
pub use monitor_stream::{Monitor, MonitorStream};
pub use scoped_stream::ScopedStream;
#[cfg(feature = "std")]
pub use utils::FromStd;
pub use utils::{BorrowedBuf, BorrowedCursor, Cursor, IoSliceMut};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: io::ErrorKind,
    message: Cow<'static, str>,
    eof: bool,
}

impl Error {
    pub fn new(kind: ErrorKind, msg: impl Into<Cow<'static, str>>) -> Self {
        Self { kind, message: msg.into(), eof: false }
    }

    pub fn other(msg: impl Into<Cow<'static, str>>) -> Self {
        Self { kind: io::ErrorKind::Other, message: msg.into(), eof: false }
    }

    pub fn eof(msg: impl Into<Cow<'static, str>>) -> Self {
        Self { kind: io::ErrorKind::Other, message: msg.into(), eof: true }
    }

    pub fn kind(&self) -> embedded_io::ErrorKind {
        self.kind
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

impl io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        self.kind
    }
}

impl core::error::Error for Error {}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::write!(f, "{}", self.message)
    }
}

impl From<io::ErrorKind> for Error {
    fn from(value: io::ErrorKind) -> Self {
        Self { kind: value, message: "no message".into(), eof: false }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self {
            kind: value.kind().into(),
            message: format!("{}", value).into(),
            eof: value.kind() == std::io::ErrorKind::UnexpectedEof,
        }
    }
}

#[async_trait]
pub trait Read: ErrorType {
    async fn read(&mut self, buf: &mut [u8]) -> core::result::Result<usize, Self::Error>;

    async fn read_exact(
        &mut self,
        mut buf: &mut [u8],
    ) -> core::result::Result<(), ReadExactError<Self::Error>> {
        while !buf.is_empty() {
            match self.read(buf).await {
                Ok(0) => break,
                Ok(n) => buf = &mut buf[n..],
                Err(e) => return Err(ReadExactError::Other(e)),
            }
        }
        if buf.is_empty() { Ok(()) } else { Err(ReadExactError::UnexpectedEof) }
    }

    async fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> Result<usize>
    where
        Error: core::convert::From<<Self as embedded_io::ErrorType>::Error>,
    {
        Ok(default_read_vectored(self, bufs).await?)
    }

    async fn read_to_end(&mut self, buffer: &mut Vec<u8>) -> Result<usize>
    where
        Error: core::convert::From<<Self as embedded_io::ErrorType>::Error>,
    {
        Ok(default_read_to_end(self, buffer, None).await?)
    }
}

#[async_trait]
pub trait BufRead: Read {
    async fn fill_buf(&mut self) -> core::result::Result<&[u8], Self::Error>;

    fn consume(&mut self, amt: usize);
}

#[async_trait]
pub trait Seek: ErrorType {
    async fn seek(&mut self, spec: SeekFrom) -> core::result::Result<u64, Self::Error>;

    async fn rewind(&mut self) -> core::result::Result<(), Self::Error> {
        self.seek(SeekFrom::Start(0)).await?;
        Ok(())
    }

    async fn stream_position(&mut self) -> core::result::Result<u64, Self::Error> {
        self.seek(SeekFrom::Current(0)).await
    }
}

#[async_trait]
impl<T: ?Sized + Read + Send> Read for &mut T {
    #[inline]
    async fn read(&mut self, buf: &mut [u8]) -> core::result::Result<usize, Self::Error> {
        T::read(self, buf).await
    }

    #[inline]
    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> core::result::Result<(), ReadExactError<Self::Error>> {
        T::read_exact(self, buf).await
    }
}

#[async_trait]
impl<T: ?Sized + BufRead + Send> BufRead for &mut T {
    #[inline]
    async fn fill_buf(&mut self) -> core::result::Result<&[u8], Self::Error> {
        T::fill_buf(self).await
    }

    #[inline]
    fn consume(&mut self, amt: usize) {
        T::consume(self, amt);
    }
}

#[async_trait]
impl<T: ?Sized + Seek + Send> Seek for &mut T {
    #[inline]
    async fn seek(&mut self, pos: SeekFrom) -> core::result::Result<u64, Self::Error> {
        T::seek(self, pos).await
    }

    #[inline]
    async fn rewind(&mut self) -> core::result::Result<(), Self::Error> {
        T::rewind(self).await
    }

    #[inline]
    async fn stream_position(&mut self) -> core::result::Result<u64, Self::Error> {
        T::stream_position(self).await
    }
}

/// `MediaSource` is a composite trait of [`std::io::Read`] and [`std::io::Seek`]. A source *must*
/// implement this trait to be used by [`MediaSourceStream`].
///
/// Despite requiring the [`std::io::Seek`] trait, seeking is an optional capability that can be
/// queried at runtime.
#[async_trait]
pub trait MediaSource: Read + Seek + Send {
    /// Returns if the source is seekable. This may be an expensive operation.
    async fn is_seekable(&self) -> bool;

    /// Returns the length in bytes, if available. This may be an expensive operation.
    async fn byte_len(&self) -> Option<u64>;
}

#[cfg(feature = "std")]
#[async_trait]
impl MediaSource for FromStd<std::fs::File> {
    /// Returns if the `std::io::File` backing the `MediaSource` is seekable.
    ///
    /// Note: This operation involves querying the underlying file descriptor for information and
    /// may be moderately expensive. Therefore it is recommended to cache this value if used often.
    async fn is_seekable(&self) -> bool {
        // If the file's metadata is available, and the file is a regular file (i.e., not a FIFO,
        // etc.), then the MediaSource will be seekable. Otherwise assume it is not. Note that
        // metadata() follows symlinks.
        match self.inner().metadata() {
            Ok(metadata) => metadata.is_file(),
            _ => false,
        }
    }

    /// Returns the length in bytes of the `std::io::File` backing the `MediaSource`.
    ///
    /// Note: This operation involves querying the underlying file descriptor for information and
    /// may be moderately expensive. Therefore it is recommended to cache this value if used often.
    async fn byte_len(&self) -> Option<u64> {
        match self.inner().metadata() {
            Ok(metadata) => Some(metadata.len()),
            _ => None,
        }
    }
}

#[async_trait]
impl<T: core::convert::AsRef<[u8]> + Send + Sync> MediaSource for Cursor<T> {
    /// Always returns true since a `io::Cursor<u8>` is always seekable.
    async fn is_seekable(&self) -> bool {
        true
    }

    /// Returns the length in bytes of the `io::Cursor<u8>` backing the `MediaSource`.
    async fn byte_len(&self) -> Option<u64> {
        // Get the underlying container, usually &Vec<T>.
        let inner = self.get_ref();
        // Get slice from the underlying container, &[T], for the len() function.
        Some(inner.as_ref().len() as u64)
    }
}

/// `ReadOnlySource` wraps any source implementing [`embedded_io::Read`] in an unseekable
/// [`MediaSource`].
pub struct ReadOnlySource<R: Read> {
    inner: R,
}

impl<R: Read + Send> ReadOnlySource<R> {
    /// Instantiates a new `ReadOnlySource<R>` by taking ownership and wrapping the provided
    /// `Read`er.
    pub fn new(inner: R) -> Self {
        ReadOnlySource { inner }
    }

    /// Gets a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Unwraps this `ReadOnlySource<R>`, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> io::ErrorType for ReadOnlySource<R> {
    type Error = Error;
}

#[async_trait]
impl<R: Read + Send + Sync> MediaSource for ReadOnlySource<R>
where
    Error: core::convert::From<<R as embedded_io::ErrorType>::Error>,
{
    async fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[async_trait]
impl<R: Read + Send> Read for ReadOnlySource<R>
where
    Error: core::convert::From<<R as embedded_io::ErrorType>::Error>,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        Ok(self.inner.read(buf).await?)
    }
}

#[async_trait]
impl<R: Read + Send> Seek for ReadOnlySource<R> {
    async fn seek(&mut self, _: io::SeekFrom) -> Result<u64> {
        Err(Error::other("source does not support seeking"))
    }
}

/// `ReadBytes` provides methods to read bytes and interpret them as little- or big-endian
/// unsigned integers or floating-point values of standard widths.
pub trait ReadBytes: Send {
    /// Reads a single byte from the stream and returns it or an error.
    fn read_byte(&mut self) -> impl Future<Output = Result<u8>> + Send;

    /// Reads two bytes from the stream and returns them in read-order or an error.
    fn read_double_bytes(&mut self) -> impl Future<Output = Result<[u8; 2]>> + Send;

    /// Reads three bytes from the stream and returns them in read-order or an error.
    fn read_triple_bytes(&mut self) -> impl Future<Output = Result<[u8; 3]>> + Send;

    /// Reads four bytes from the stream and returns them in read-order or an error.
    fn read_quad_bytes(&mut self) -> impl Future<Output = Result<[u8; 4]>> + Send;

    /// Reads up-to the number of bytes required to fill buf or returns an error.
    fn read_buf(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize>> + Send;

    /// Reads exactly the number of bytes required to fill be provided buffer or returns an error.
    fn read_buf_exact(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<()>> + Send;

    /// Reads a single unsigned byte from the stream and returns it or an error.
    #[inline(always)]
    fn read_u8(&mut self) -> impl Future<Output = Result<u8>> + Send {
        async { self.read_byte().await }
    }

    /// Reads a single signed byte from the stream and returns it or an error.
    #[inline(always)]
    fn read_i8(&mut self) -> impl Future<Output = Result<i8>> + Send {
        async { Ok(self.read_byte().await? as i8) }
    }

    /// Reads two bytes from the stream and interprets them as an unsigned 16-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_u16(&mut self) -> impl Future<Output = Result<u16>> + Send {
        async { Ok(u16::from_le_bytes(self.read_double_bytes().await?)) }
    }

    /// Reads two bytes from the stream and interprets them as an signed 16-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_i16(&mut self) -> impl Future<Output = Result<i16>> + Send {
        async { Ok(i16::from_le_bytes(self.read_double_bytes().await?)) }
    }

    /// Reads two bytes from the stream and interprets them as an unsigned 16-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_u16(&mut self) -> impl Future<Output = Result<u16>> + Send {
        async { Ok(u16::from_be_bytes(self.read_double_bytes().await?)) }
    }

    /// Reads two bytes from the stream and interprets them as an signed 16-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_i16(&mut self) -> impl Future<Output = Result<i16>> + Send {
        async { Ok(i16::from_be_bytes(self.read_double_bytes().await?)) }
    }

    /// Reads three bytes from the stream and interprets them as an unsigned 24-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_u24(&mut self) -> impl Future<Output = Result<u32>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u32>()];
            buf[0..3].clone_from_slice(&self.read_triple_bytes().await?);
            Ok(u32::from_le_bytes(buf))
        }
    }

    /// Reads three bytes from the stream and interprets them as an signed 24-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_i24(&mut self) -> impl Future<Output = Result<i32>> + Send {
        async { Ok(((self.read_u24().await? << 8) as i32) >> 8) }
    }

    /// Reads three bytes from the stream and interprets them as an unsigned 24-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_u24(&mut self) -> impl Future<Output = Result<u32>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u32>()];
            buf[0..3].clone_from_slice(&self.read_triple_bytes().await?);
            Ok(u32::from_be_bytes(buf) >> 8)
        }
    }

    /// Reads three bytes from the stream and interprets them as an signed 24-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_i24(&mut self) -> impl Future<Output = Result<i32>> + Send {
        async { Ok(((self.read_be_u24().await? << 8) as i32) >> 8) }
    }

    /// Reads four bytes from the stream and interprets them as an unsigned 32-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_u32(&mut self) -> impl Future<Output = Result<u32>> + Send {
        async { Ok(u32::from_le_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads four bytes from the stream and interprets them as an signed 32-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_i32(&mut self) -> impl Future<Output = Result<i32>> + Send {
        async { Ok(i32::from_le_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads four bytes from the stream and interprets them as an unsigned 32-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_u32(&mut self) -> impl Future<Output = Result<u32>> + Send {
        async { Ok(u32::from_be_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads four bytes from the stream and interprets them as a signed 32-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_i32(&mut self) -> impl Future<Output = Result<i32>> + Send {
        async { Ok(i32::from_be_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads eight bytes from the stream and interprets them as an unsigned 64-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_u64(&mut self) -> impl Future<Output = Result<u64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(u64::from_le_bytes(buf))
        }
    }

    /// Reads eight bytes from the stream and interprets them as an signed 64-bit little-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_i64(&mut self) -> impl Future<Output = Result<i64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<i64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(i64::from_le_bytes(buf))
        }
    }

    /// Reads eight bytes from the stream and interprets them as an unsigned 64-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_u64(&mut self) -> impl Future<Output = Result<u64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(u64::from_be_bytes(buf))
        }
    }

    /// Reads eight bytes from the stream and interprets them as an signed 64-bit big-endian
    /// integer or returns an error.
    #[inline(always)]
    fn read_be_i64(&mut self) -> impl Future<Output = Result<i64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<i64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(i64::from_be_bytes(buf))
        }
    }

    /// Reads four bytes from the stream and interprets them as a 32-bit little-endian IEEE-754
    /// floating-point value.
    #[inline(always)]
    fn read_f32(&mut self) -> impl Future<Output = Result<f32>> + Send {
        async { Ok(f32::from_le_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads four bytes from the stream and interprets them as a 32-bit big-endian IEEE-754
    /// floating-point value.
    #[inline(always)]
    fn read_be_f32(&mut self) -> impl Future<Output = Result<f32>> + Send {
        async { Ok(f32::from_be_bytes(self.read_quad_bytes().await?)) }
    }

    /// Reads four bytes from the stream and interprets them as a 64-bit little-endian IEEE-754
    /// floating-point value.
    #[inline(always)]
    fn read_f64(&mut self) -> impl Future<Output = Result<f64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(f64::from_le_bytes(buf))
        }
    }

    /// Reads four bytes from the stream and interprets them as a 64-bit big-endian IEEE-754
    /// floating-point value.
    #[inline(always)]
    fn read_be_f64(&mut self) -> impl Future<Output = Result<f64>> + Send {
        async {
            let mut buf = [0u8; core::mem::size_of::<u64>()];
            self.read_buf_exact(&mut buf).await?;
            Ok(f64::from_be_bytes(buf))
        }
    }

    /// Reads up-to the number of bytes requested, and returns a boxed slice of the data or an
    /// error.
    fn read_boxed_slice(&mut self, len: usize) -> impl Future<Output = Result<Box<[u8]>>> + Send {
        async move {
            let mut buf = vec![0u8; len];
            let actual_len = self.read_buf(&mut buf).await?;
            buf.truncate(actual_len);
            Ok(buf.into_boxed_slice())
        }
    }

    /// Reads exactly the number of bytes requested, and returns a boxed slice of the data or an
    /// error.
    fn read_boxed_slice_exact(
        &mut self,
        len: usize,
    ) -> impl Future<Output = Result<Box<[u8]>>> + Send {
        async move {
            let mut buf = vec![0u8; len];
            self.read_buf_exact(&mut buf).await?;
            Ok(buf.into_boxed_slice())
        }
    }

    /// Reads bytes from the stream into a supplied buffer until a byte pattern is matched. Returns
    /// a mutable slice to the valid region of the provided buffer.
    #[inline(always)]
    fn scan_bytes<'a>(
        &mut self,
        pattern: &[u8],
        buf: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a mut [u8]>> + Send {
        async move { self.scan_bytes_aligned(pattern, 1, buf).await }
    }

    /// Reads bytes from a stream into a supplied buffer until a byte patter is matched on an
    /// aligned byte boundary. Returns a mutable slice to the valid region of the provided buffer.
    fn scan_bytes_aligned<'a>(
        &mut self,
        pattern: &[u8],
        align: usize,
        buf: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a mut [u8]>> + Send;

    /// Ignores the specified number of bytes from the stream or returns an error.
    fn ignore_bytes(&mut self, count: u64) -> impl Future<Output = Result<()>> + Send;

    /// Gets the position of the stream.
    fn pos(&self) -> u64;
}

impl<R: ReadBytes> ReadBytes for &mut R {
    #[inline(always)]
    async fn read_byte(&mut self) -> Result<u8> {
        (*self).read_byte().await
    }

    #[inline(always)]
    async fn read_double_bytes(&mut self) -> Result<[u8; 2]> {
        (*self).read_double_bytes().await
    }

    #[inline(always)]
    async fn read_triple_bytes(&mut self) -> Result<[u8; 3]> {
        (*self).read_triple_bytes().await
    }

    #[inline(always)]
    async fn read_quad_bytes(&mut self) -> Result<[u8; 4]> {
        (*self).read_quad_bytes().await
    }

    #[inline(always)]
    async fn read_buf(&mut self, buf: &mut [u8]) -> Result<usize> {
        (*self).read_buf(buf).await
    }

    #[inline(always)]
    async fn read_buf_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        (*self).read_buf_exact(buf).await
    }

    #[inline(always)]
    async fn scan_bytes_aligned<'a>(
        &mut self,
        pattern: &[u8],
        align: usize,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8]> {
        (*self).scan_bytes_aligned(pattern, align, buf).await
    }

    #[inline(always)]
    async fn ignore_bytes(&mut self, count: u64) -> Result<()> {
        (*self).ignore_bytes(count).await
    }

    #[inline(always)]
    fn pos(&self) -> u64 {
        (**self).pos()
    }
}

impl<S: SeekBuffered> SeekBuffered for &mut S {
    fn ensure_seekback_buffer(&mut self, len: usize) {
        (*self).ensure_seekback_buffer(len)
    }

    fn unread_buffer_len(&self) -> usize {
        (**self).unread_buffer_len()
    }

    fn read_buffer_len(&self) -> usize {
        (**self).read_buffer_len()
    }

    fn seek_buffered(&mut self, pos: u64) -> u64 {
        (*self).seek_buffered(pos)
    }

    fn seek_buffered_rel(&mut self, delta: isize) -> u64 {
        (*self).seek_buffered_rel(delta)
    }
}

/// `SeekBuffered` provides methods to seek within the buffered portion of a stream.
pub trait SeekBuffered {
    /// Ensures that `len` bytes will be available for backwards seeking if `len` bytes have been
    /// previously read.
    fn ensure_seekback_buffer(&mut self, len: usize);

    /// Get the number of bytes buffered but not yet read.
    ///
    /// Note: This is the maximum number of bytes that can be seeked forwards within the buffer.
    fn unread_buffer_len(&self) -> usize;

    /// Gets the number of bytes buffered and read.
    ///
    /// Note: This is the maximum number of bytes that can be seeked backwards within the buffer.
    fn read_buffer_len(&self) -> usize;

    /// Seek within the buffered data to an absolute position in the stream. Returns the position
    /// seeked to.
    fn seek_buffered(&mut self, pos: u64) -> u64;

    /// Seek within the buffered data relative to the current position in the stream. Returns the
    /// position seeked to.
    ///
    /// The range of `delta` is clamped to the inclusive range defined by
    /// `-read_buffer_len()..=unread_buffer_len()`.
    fn seek_buffered_rel(&mut self, delta: isize) -> u64;

    /// Seek backwards within the buffered data.
    ///
    /// This function is identical to [`SeekBuffered::seek_buffered_rel`] when a negative delta is
    /// provided.
    fn seek_buffered_rev(&mut self, delta: usize) {
        assert!(delta < isize::MAX as usize);
        self.seek_buffered_rel(-(delta as isize));
    }
}

impl<F: FiniteStream> FiniteStream for &mut F {
    fn byte_len(&self) -> u64 {
        (**self).byte_len()
    }

    fn bytes_read(&self) -> u64 {
        (**self).bytes_read()
    }

    fn bytes_available(&self) -> u64 {
        (**self).bytes_available()
    }
}

/// A `FiniteStream` is a stream that has a known length in bytes.
pub trait FiniteStream {
    /// Returns the length of the the stream in bytes.
    fn byte_len(&self) -> u64;

    /// Returns the number of bytes that have been read.
    fn bytes_read(&self) -> u64;

    /// Returns the number of bytes available for reading.
    fn bytes_available(&self) -> u64;
}
