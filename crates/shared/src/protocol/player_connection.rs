// The transport that carries a player's game client to this server.
//
// The netcode reads and writes the same messages whichever transport carries a player: a reliable bidirectional stream
// for the handshake and for each request, unreliable datagrams for inputs and snapshots, and a notification when the
// connection ends. QUIC (the desktop client) and WebTransport over HTTP/3 (a browser page) bind that one protocol here,
// so the server's gameplay code meets either of them through these types instead of handling both separately.

use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::Result;
use bytes::Bytes;
use quinn::Connection;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wtransport::Connection as WebTransportConnection;

// The application close code the game has always ended a player's connection with.
const PLAYER_CLOSE_ERROR_CODE: u32 = 500;

#[derive(Clone)]
pub enum PlayerConnection {
    Quic(Connection),
    WebTransport(WebTransportConnection),
}

impl PlayerConnection {
    pub fn remote_address(&self) -> SocketAddr {
        match self {
            Self::Quic(connection) => connection.remote_address(),
            Self::WebTransport(connection) => connection.remote_address(),
        }
    }

    // A stream the client opened. The handshake is the first one; chat subscriptions and script calls follow.
    pub async fn accept_bi(&self) -> Result<(PlayerSendStream, PlayerRecvStream)> {
        match self {
            Self::Quic(connection) => {
                let (send_stream, recv_stream) = connection.accept_bi().await?;
                Ok((PlayerSendStream::Quic(send_stream), PlayerRecvStream::Quic(recv_stream)))
            }
            Self::WebTransport(connection) => {
                let (send_stream, recv_stream) = connection.accept_bi().await?;
                Ok((PlayerSendStream::WebTransport(send_stream), PlayerRecvStream::WebTransport(recv_stream)))
            }
        }
    }

    pub async fn open_bi(&self) -> Result<(PlayerSendStream, PlayerRecvStream)> {
        match self {
            Self::Quic(connection) => {
                let (send_stream, recv_stream) = connection.open_bi().await?;
                Ok((PlayerSendStream::Quic(send_stream), PlayerRecvStream::Quic(recv_stream)))
            }
            Self::WebTransport(connection) => {
                let (send_stream, recv_stream) = connection.open_bi().await?.await?;
                Ok((PlayerSendStream::WebTransport(send_stream), PlayerRecvStream::WebTransport(recv_stream)))
            }
        }
    }

    pub async fn open_uni(&self) -> Result<PlayerSendStream> {
        match self {
            Self::Quic(connection) => Ok(PlayerSendStream::Quic(connection.open_uni().await?)),
            Self::WebTransport(connection) => Ok(PlayerSendStream::WebTransport(connection.open_uni().await?.await?)),
        }
    }

    // Resolves when the connection ends, whichever side ended it.
    pub async fn closed(&self) {
        match self {
            Self::Quic(connection) => {
                connection.closed().await;
            }
            Self::WebTransport(connection) => {
                connection.closed().await;
            }
        }
    }

    pub fn close(&self, reason: &str) {
        match self {
            Self::Quic(connection) => connection.close(quinn::VarInt::from_u32(PLAYER_CLOSE_ERROR_CODE), reason.as_bytes()),
            Self::WebTransport(connection) => connection.close(wtransport::VarInt::from_u32(PLAYER_CLOSE_ERROR_CODE), reason.as_bytes()),
        }
    }
}

// ----------------- The unreliable channel -----------------

// What the netcode needs from a connection's datagrams: the biggest message that fits, how far away the player is,
// and one message each way.
// Only ever used as a bound on a concrete connection, never as a trait object.
#[allow(async_fn_in_trait)]
pub trait DatagramConnection {
    fn send_datagram_bytes(&self, bytes: Bytes) -> Result<()>;
    async fn recv_datagram_bytes(&self) -> Result<Bytes>;
    fn max_datagram_size(&self) -> Option<usize>;
    fn rtt(&self) -> Duration;
    // Whether the connection has ended, so a read that fails because the player left is told apart from one that failed
    // for another reason.
    fn is_closed(&self) -> bool;
}

impl DatagramConnection for Connection {
    fn send_datagram_bytes(&self, bytes: Bytes) -> Result<()> {
        self.send_datagram(bytes)?;
        Ok(())
    }

    async fn recv_datagram_bytes(&self) -> Result<Bytes> {
        Ok(self.read_datagram().await?)
    }

    fn max_datagram_size(&self) -> Option<usize> {
        Connection::max_datagram_size(self)
    }

    fn rtt(&self) -> Duration {
        Connection::rtt(self)
    }

    fn is_closed(&self) -> bool {
        self.close_reason().is_some()
    }
}

impl DatagramConnection for WebTransportConnection {
    fn send_datagram_bytes(&self, bytes: Bytes) -> Result<()> {
        self.send_datagram(bytes)?;
        Ok(())
    }

    async fn recv_datagram_bytes(&self) -> Result<Bytes> {
        Ok(self.receive_datagram().await?.payload())
    }

    fn max_datagram_size(&self) -> Option<usize> {
        // WebTransport subtracts its own datagram header, so both bindings report what a message may be.
        WebTransportConnection::max_datagram_size(self)
    }

    fn rtt(&self) -> Duration {
        WebTransportConnection::rtt(self)
    }

    fn is_closed(&self) -> bool {
        // WebTransport runs its HTTP/3 session over a QUIC connection of its own.
        self.quic_connection().close_reason().is_some()
    }
}

impl DatagramConnection for PlayerConnection {
    fn send_datagram_bytes(&self, bytes: Bytes) -> Result<()> {
        match self {
            Self::Quic(connection) => connection.send_datagram_bytes(bytes),
            Self::WebTransport(connection) => connection.send_datagram_bytes(bytes),
        }
    }

    async fn recv_datagram_bytes(&self) -> Result<Bytes> {
        match self {
            Self::Quic(connection) => connection.recv_datagram_bytes().await,
            Self::WebTransport(connection) => connection.recv_datagram_bytes().await,
        }
    }

    fn max_datagram_size(&self) -> Option<usize> {
        match self {
            Self::Quic(connection) => connection.max_datagram_size(),
            Self::WebTransport(connection) => connection.max_datagram_size(),
        }
    }

    fn rtt(&self) -> Duration {
        match self {
            Self::Quic(connection) => connection.rtt(),
            Self::WebTransport(connection) => connection.rtt(),
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            Self::Quic(connection) => connection.is_closed(),
            Self::WebTransport(connection) => connection.is_closed(),
        }
    }
}

// ----------------- The reliable streams -----------------

// Both bindings' streams speak tokio's AsyncRead and AsyncWrite, which is what the message framing uses. WebTransport's
// send stream has no synchronous finish and no `stopped`, so those two live here.
pub enum PlayerSendStream {
    Quic(quinn::SendStream),
    WebTransport(wtransport::SendStream),
}

pub enum PlayerRecvStream {
    Quic(quinn::RecvStream),
    WebTransport(wtransport::RecvStream),
}

impl PlayerSendStream {
    // Tells the peer that no more messages follow this one.
    pub async fn finish(&mut self) -> Result<()> {
        match self {
            Self::Quic(send_stream) => Ok(send_stream.finish()?),
            Self::WebTransport(send_stream) => Ok(send_stream.finish().await?),
        }
    }

    // Resolves when the peer stops reading this stream, which is how a chat subscriber that left is noticed. The future
    // owns its own handle, so a sender can keep writing on the stream while it waits.
    pub fn stopped(&self) -> impl Future<Output = Result<()>> + Send + use<> {
        // A WebTransport send stream is a QUIC stream with WebTransport's framing on top, so both stop the same way.
        fn awaitStopped(player_send_stream: &PlayerSendStream) -> impl Future<Output = Result<()>> + Send + use<> {
            let stopped = match player_send_stream {
                PlayerSendStream::Quic(send_stream) => send_stream.stopped(),
                PlayerSendStream::WebTransport(send_stream) => send_stream.quic_stream().stopped(),
            };
            async move {
                stopped.await?;
                Ok(())
            }
        }
        awaitStopped(self)
    }
}

impl AsyncWrite for PlayerSendStream {
    fn poll_write(self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Quic(send_stream) => AsyncWrite::poll_write(Pin::new(send_stream), context, buffer),
            Self::WebTransport(send_stream) => AsyncWrite::poll_write(Pin::new(send_stream), context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Quic(send_stream) => AsyncWrite::poll_flush(Pin::new(send_stream), context),
            Self::WebTransport(send_stream) => AsyncWrite::poll_flush(Pin::new(send_stream), context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Quic(send_stream) => AsyncWrite::poll_shutdown(Pin::new(send_stream), context),
            Self::WebTransport(send_stream) => AsyncWrite::poll_shutdown(Pin::new(send_stream), context),
        }
    }
}

impl AsyncRead for PlayerRecvStream {
    fn poll_read(self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Quic(recv_stream) => AsyncRead::poll_read(Pin::new(recv_stream), context, buffer),
            Self::WebTransport(recv_stream) => AsyncRead::poll_read(Pin::new(recv_stream), context, buffer),
        }
    }
}
