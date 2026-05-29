use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::protocol::packet::{
    encode_message, PacketFrameError, PacketHeader, PacketHeaderError, PacketType,
    PACKET_HEADER_LEN,
};

pub(crate) struct TlsPreloginStream<S> {
    stream: S,
    handshake: bool,
    header_buf: [u8; PACKET_HEADER_LEN],
    header_pos: usize,
    read_remaining: usize,
    write_buf: Vec<u8>,
}

impl<S> TlsPreloginStream<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self {
            stream,
            handshake: false,
            header_buf: [0; PACKET_HEADER_LEN],
            header_pos: 0,
            read_remaining: 0,
            write_buf: Vec::new(),
        }
    }

    pub(crate) fn start_handshake(&mut self) {
        self.handshake = true;
    }

    pub(crate) fn finish_handshake(&mut self) {
        self.handshake = false;
    }
}

impl<S> std::fmt::Debug for TlsPreloginStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsPreloginStream")
            .field("handshake", &self.handshake)
            .field("read_remaining", &self.read_remaining)
            .field("write_buf_len", &self.write_buf.len())
            .finish_non_exhaustive()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsPreloginStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.handshake {
            return Pin::new(&mut self.stream).poll_read(cx, buf);
        }

        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        if self.read_remaining == 0 {
            while self.header_pos < PACKET_HEADER_LEN {
                let mut scratch = [0; PACKET_HEADER_LEN];
                let remaining = PACKET_HEADER_LEN - self.header_pos;
                let mut header_read = ReadBuf::new(&mut scratch[..remaining]);
                ready!(Pin::new(&mut self.stream).poll_read(cx, &mut header_read))?;

                let read = header_read.filled().len();
                if read == 0 {
                    let message = if self.header_pos == 0 {
                        "SQL Server closed the connection before sending a TDS PRELOGIN packet during TLS handshake"
                    } else {
                        "SQL Server closed the connection in the middle of a TDS PRELOGIN packet header during TLS handshake"
                    };
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof, message)));
                }

                let header_pos = self.header_pos;
                self.header_buf[header_pos..header_pos + read]
                    .copy_from_slice(header_read.filled());
                self.header_pos += read;
            }

            let header = PacketHeader::decode(&self.header_buf).map_err(packet_header_io_error)?;
            if header.packet_type != PacketType::PRE_LOGIN {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "expected TLS handshake bytes in PRELOGIN packet, got packet type 0x{:02x}",
                        header.packet_type.code()
                    ),
                )));
            }

            self.read_remaining = usize::from(header.length)
                .checked_sub(PACKET_HEADER_LEN)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid TDS packet length")
                })?;
            self.header_pos = 0;
        }

        let max_read = std::cmp::min(self.read_remaining, buf.remaining());
        let mut limited_buf = buf.take(max_read);
        ready!(Pin::new(&mut self.stream).poll_read(cx, &mut limited_buf))?;

        let read = limited_buf.filled().len();
        if read == 0 && self.read_remaining > 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "SQL Server closed the connection in the middle of a TDS PRELOGIN TLS payload",
            )));
        }

        buf.advance(read);
        self.read_remaining -= read;

        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsPreloginStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.handshake {
            return Pin::new(&mut self.stream).poll_write(cx, buf);
        }

        self.write_buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.handshake && !self.write_buf.is_empty() {
            let payload = std::mem::take(&mut self.write_buf);
            self.write_buf =
                wrap_prelogin_tls_payload(&payload, 4096).map_err(packet_frame_error)?;

            while !self.write_buf.is_empty() {
                let write_buf = std::mem::take(&mut self.write_buf);
                let written = ready!(Pin::new(&mut self.stream).poll_write(cx, &write_buf))?;
                if written == 0 {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write TLS handshake packet",
                    )));
                }

                if written < write_buf.len() {
                    self.write_buf.extend_from_slice(&write_buf[written..]);
                }
            }
        }

        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

fn wrap_prelogin_tls_payload(
    payload: &[u8],
    packet_size: usize,
) -> Result<Vec<u8>, PacketFrameError> {
    encode_message(PacketType::PRE_LOGIN, payload, packet_size)
}

fn packet_header_io_error(error: PacketHeaderError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn packet_frame_error(error: PacketFrameError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("failed to wrap TLS handshake bytes in a TDS PRELOGIN packet: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_tls_handshake_bytes_as_prelogin_packets() {
        let packet = wrap_prelogin_tls_payload(b"hello", 512).unwrap();
        let header = PacketHeader::decode(&packet[..PACKET_HEADER_LEN]).unwrap();

        assert_eq!(PacketType::PRE_LOGIN, header.packet_type);
        assert_eq!(PACKET_HEADER_LEN + 5, usize::from(header.length));
        assert_eq!(b"hello", &packet[PACKET_HEADER_LEN..]);
    }
}
