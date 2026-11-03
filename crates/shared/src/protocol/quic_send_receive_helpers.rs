use bincode::Options;
use bytes::Bytes;
use quinn::{Connection, RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::AsyncReadExt;

use crate::protocol::bincode_config::get_bincode_config;

// ---------------------- TCP ----------------------

pub const MAX_INITIAL_WORLD_MESSAGE_BYTES: u32 = u32::MAX;
pub const MAX_TCP_MESSAGE_BYTES: u32 = 4 * 1024 * 1024;

// Each request gets its own reliable stream and one response. Subscriptions use sendTcpMessage directly.
pub async fn sendTcpRequest<Request, Response>(connection: &Connection, request: &Request) -> anyhow::Result<Response>
where
    Request: Serialize,
    Response: DeserializeOwned,
{
    let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
    sendTcpMessage(&mut send_stream, request).await?;
    send_stream.finish()?;
    awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await
}

pub async fn sendTcpMessage<T>(send_stream: &mut SendStream, message: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let bincode_config = get_bincode_config();
    let bytes = bincode_config.serialize(message)?;

    // write the length as a u32 prefix
    let message_num_bytes = u32::try_from(bytes.len())?;
    send_stream.write_all(&message_num_bytes.to_be_bytes()).await?;

    // write the message bytes afterwards
    send_stream.write_all(&bytes).await?;
    Ok(())
}

pub async fn awaitTcpMessage<T>(recv_stream: &mut RecvStream) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    awaitTcpMessageMaxBytes(recv_stream, 1024).await
}

pub async fn awaitTcpMessageMaxBytes<T>(recv_stream: &mut RecvStream, max_bytes: u32) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    // read the length - each TCP message starts with a 4-byte length prefix, saying how long the actual message is
    let message_num_bytes = recv_stream.read_u32().await?;
    if message_num_bytes > max_bytes {
        anyhow::bail!("TCP message exceeded max size: {} > {}", message_num_bytes, max_bytes);
    }

    // message_buffer is the actual message bytes
    let mut bytes = vec![0u8; message_num_bytes as usize];
    recv_stream.read_exact(&mut bytes).await?;

    // turn bytes into message
    let bincode_config = get_bincode_config();
    let message = bincode_config.deserialize::<T>(&bytes)?;
    Ok(message)
}

// ---------------------- UDP ----------------------

pub fn sendDatagramMessage<T>(connection: &Connection, message: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let bincode_config = get_bincode_config();
    let bytes = Bytes::from(bincode_config.serialize(message)?);
    connection.send_datagram(bytes)?;
    Ok(())
}

pub async fn awaitDatagramMessage<T>(connection: &Connection) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let bytes = connection.read_datagram().await?;
    let bincode_config = get_bincode_config();
    let message = bincode_config.deserialize::<T>(&bytes)?;
    Ok(message)
}

// ---------------------- Ping ----------------------

// A player's round trip to the server, as measured on their connection. The connection measures it from its first packets, so it's known by the time a player joins.
pub fn getPingMs(connection: &Connection) -> u16 {
    u16::try_from(connection.rtt().as_millis()).unwrap_or(u16::MAX)
}
