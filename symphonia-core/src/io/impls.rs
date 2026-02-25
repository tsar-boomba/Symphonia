use async_trait::async_trait;

use super::{BufRead, Read, Seek, SeekFrom};
use alloc::boxed::Box;

/// Read is implemented for `&[u8]` by copying from the slice.
///
/// Note that reading updates the slice to point to the yet unread part.
/// The slice will be empty when EOF is reached.
#[async_trait]
impl Read for &[u8] {
    #[inline]
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let amt = core::cmp::min(buf.len(), self.len());
        let (a, b) = self.split_at(amt);

        // First check if the amount of bytes we want to read is small:
        // `copy_from_slice` will generally expand to a call to `memcpy`, and
        // for a single byte the overhead is significant.
        if amt == 1 {
            buf[0] = a[0];
        }
        else {
            buf[..amt].copy_from_slice(a);
        }

        *self = b;
        Ok(amt)
    }

    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), embedded_io::ReadExactError<Self::Error>> {
        if self.len() < buf.len() {
            return Err(super::ReadExactError::UnexpectedEof);
        }
        self.read(buf).await?;
        Ok(())
    }
}

#[async_trait]
impl BufRead for &[u8] {
    #[inline]
    async fn fill_buf(&mut self) -> Result<&[u8], Self::Error> {
        Ok(*self)
    }

    #[inline]
    fn consume(&mut self, amt: usize) {
        *self = &self[amt..];
    }
}

#[async_trait]
impl<T: ?Sized + Read + Send> Read for Box<T> {
    #[inline]
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        T::read(self, buf).await
    }

    #[inline]
    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), super::ReadExactError<Self::Error>> {
        T::read_exact(self, buf).await
    }
}

#[async_trait]
impl<T: ?Sized + BufRead + Send> BufRead for Box<T> {
    #[inline]
    async fn fill_buf(&mut self) -> Result<&[u8], Self::Error> {
        T::fill_buf(self).await
    }

    #[inline]
    fn consume(&mut self, amt: usize) {
        T::consume(self, amt);
    }
}

#[async_trait]
impl<T: ?Sized + Seek + Send> Seek for Box<T> {
    #[inline]
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        T::seek(self, pos).await
    }

    #[inline]
    async fn rewind(&mut self) -> Result<(), Self::Error> {
        T::rewind(self).await
    }

    #[inline]
    async fn stream_position(&mut self) -> Result<u64, Self::Error> {
        T::stream_position(self).await
    }
}
