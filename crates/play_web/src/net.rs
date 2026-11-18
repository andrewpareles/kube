// The page's connection to a game server.
//
// A browser reaches the server over WebTransport, which is HTTP/3 with reliable streams and unreliable datagrams. It
// carries the same messages the desktop client sends over QUIC: one reliable stream for the handshake, then inputs, acks,
// pings and snapshots as datagrams. This module is the browser's binding of that protocol, the counterpart of
// `shared::protocol::player_connection` on the server.

use bincode::Options;
use game_types::consts::CLIENT_SERVER_PROTOCOL_VERSION;
use js_sys::{Reflect, Uint8Array};
use shared::protocol::{
    bincode_config::get_bincode_config,
    messages::{ClientHandshakeTCP, ClientHelloMessage, ClientMessageUDP, ServerHandshakeTCP, ServerInitialWorldMessage, ServerMessageUDP},
};
use tokio::sync::mpsc;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    ReadableStream, ReadableStreamDefaultReader, WebTransport, WebTransportBidirectionalStream, WebTransportHash, WebTransportOptions, WritableStream,
    WritableStreamDefaultWriter,
};

// The handshake message carries the whole stored world, so the page accepts messages as large as the server sends.
const MAX_HANDSHAKE_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

// What the frame loop has with the connection: messages waiting for it, a queue it writes to, and why the game ended.
pub struct ServerConnection {
    incoming_rx: mpsc::UnboundedReceiver<ServerMessageUDP>,
    outgoing_tx: mpsc::UnboundedSender<ClientMessageUDP>,
    ended_rx: mpsc::UnboundedReceiver<String>,
}

impl ServerConnection {
    // Joins a game and reports the world the server is running. `certificate_hash` is the SHA-256 of the server's
    // certificate, which is how a page accepts a server whose certificate no authority signed.
    pub async fn join(server_url: &str, play_auth: &str, certificate_hash: Option<Vec<u8>>) -> Result<(Self, ServerInitialWorldMessage), String> {
        let options = WebTransportOptions::new();
        if let Some(certificate_hash) = certificate_hash {
            let hash = WebTransportHash::new();
            hash.set_algorithm("sha-256");
            hash.set_value(&Uint8Array::from(certificate_hash.as_slice()));
            options.set_server_certificate_hashes(&[hash]);
        }
        let transport = WebTransport::new_with_options(server_url, &options).map_err(describe_js_error)?;
        JsFuture::from(transport.ready()).await.map_err(describe_js_error)?;
        let initial_world = handshake(&transport, play_auth).await?;

        let datagrams = transport.datagrams();
        let mut datagram_reader = DatagramReader {
            reader: datagrams.readable().get_reader().unchecked_into(),
            transport: transport.clone(),
        };
        let datagram_writer = DatagramWriter {
            writer: datagrams.writable().get_writer().map_err(describe_js_error)?,
        };
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel();
        let (ended_tx, ended_rx) = mpsc::unbounded_channel::<String>();
        let ended_tx_of_reader = ended_tx.clone();

        // Reading and writing run on their own tasks, so the page draws a frame whether or not a datagram arrived.
        spawn_local(async move {
            let mut datagram_writer = datagram_writer;
            while let Some(message) = outgoing_rx.recv().await {
                if let Err(error) = datagram_writer.send(message).await {
                    let _ = ended_tx.send(error);
                    return;
                }
            }
        });
        spawn_local(async move {
            loop {
                match datagram_reader.await_next().await {
                    Ok(message) => {
                        if incoming_tx.send(message).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let ended_reason = datagram_reader.await_closed_reason().await;
                        let _ = ended_tx_of_reader.send(ended_reason);
                        return;
                    }
                }
            }
        });

        Ok((
            Self {
                incoming_rx,
                outgoing_tx,
                ended_rx,
            },
            initial_world,
        ))
    }

    // Every message that arrived since the last call, which the frame loop applies before stepping.
    pub fn take_incoming(&mut self) -> Vec<ServerMessageUDP> {
        std::iter::from_fn(|| self.incoming_rx.try_recv().ok()).collect()
    }

    pub fn send(&self, message: ClientMessageUDP) {
        let _ = self.outgoing_tx.send(message);
    }

    // Why the game ended, once it has, so the page can say so.
    pub fn take_ended_reason(&mut self) -> Option<String> {
        self.ended_rx.try_recv().ok()
    }
}

// -------------------------- The handshake stream --------------------------

// The server expects its hello on one reliable bidirectional stream, then answers with the world and waits for the page
// to say it is ready for datagrams.
async fn handshake(transport: &WebTransport, play_auth: &str) -> Result<ServerInitialWorldMessage, String> {
    let stream: WebTransportBidirectionalStream = JsFuture::from(transport.create_bidirectional_stream()).await.map_err(describe_js_error)?;
    let web_transport_send_stream = stream.writable();
    let web_transport_receive_stream = stream.readable();
    let writable_stream: &WritableStream = web_transport_send_stream.as_ref();
    let readable_stream: &ReadableStream = web_transport_receive_stream.as_ref();
    let mut handshake_stream = HandshakeStream {
        writer: writable_stream.get_writer().map_err(describe_js_error)?,
        reader: readable_stream.get_reader().unchecked_into(),
        buffered: Vec::new(),
    };

    handshake_stream
        .write_message(&ClientHandshakeTCP::Hello(ClientHelloMessage {
            client_server_protocol_version: CLIENT_SERVER_PROTOCOL_VERSION,
            client_app_version: env!("CARGO_PKG_VERSION").to_string(),
            play_auth: play_auth.to_string(),
        }))
        .await?;
    let ServerHandshakeTCP::InitialWorld(initial_world) = handshake_stream.read_message::<ServerHandshakeTCP>().await?;
    handshake_stream.write_message(&ClientHandshakeTCP::ReadyForUDPMessages).await?;
    // The server stops reading this stream once it has the ready message, and the browser reports that as a rejected
    // close, so closing is best effort.
    let _ = JsFuture::from(handshake_stream.writer.close()).await;
    Ok(initial_world)
}

struct HandshakeStream {
    writer: WritableStreamDefaultWriter,
    reader: ReadableStreamDefaultReader,
    buffered: Vec<u8>,
}

impl HandshakeStream {
    // Each message is its length as four big-endian bytes, then the message itself, which is what the server reads.
    async fn write_message<T: serde::Serialize>(&mut self, message: &T) -> Result<(), String> {
        let bytes = get_bincode_config().serialize(message).map_err(|error| error.to_string())?;
        let mut framed = (u32::try_from(bytes.len()).map_err(|_| "The handshake message is too large.")?)
            .to_be_bytes()
            .to_vec();
        framed.extend_from_slice(&bytes);
        JsFuture::from(WritableStreamDefaultWriter::write_with_chunk(
            &self.writer,
            &Uint8Array::from(framed.as_slice()),
        ))
        .await
        .map_err(describe_js_error)?;
        Ok(())
    }

    async fn read_message<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, String> {
        self.fill_until(4).await?;
        let length = u32::from_be_bytes([self.buffered[0], self.buffered[1], self.buffered[2], self.buffered[3]]) as usize;
        if length > MAX_HANDSHAKE_MESSAGE_BYTES {
            return Err("The server sent a message too large for this page.".to_string());
        }
        self.fill_until(4 + length).await?;
        let message = get_bincode_config()
            .deserialize(&self.buffered[4..4 + length])
            .map_err(|error| error.to_string())?;
        self.buffered.drain(..4 + length);
        Ok(message)
    }

    async fn fill_until(&mut self, length: usize) -> Result<(), String> {
        while self.buffered.len() < length {
            self.buffered.extend_from_slice(&read_chunk(&self.reader).await?);
        }
        Ok(())
    }
}

// -------------------------- Datagrams --------------------------

struct DatagramReader {
    reader: ReadableStreamDefaultReader,
    transport: WebTransport,
}

impl DatagramReader {
    async fn await_next(&mut self) -> Result<ServerMessageUDP, String> {
        let bytes = read_chunk(&self.reader).await?;
        get_bincode_config().deserialize(&bytes).map_err(|error| error.to_string())
    }

    // The reason the server ended the session, for the page to show.
    async fn await_closed_reason(&self) -> String {
        match JsFuture::from(self.transport.closed()).await {
            Ok(close_info) => Reflect::get(&close_info, &JsValue::from_str("reason"))
                .ok()
                .and_then(|reason| reason.as_string())
                .filter(|reason| !reason.is_empty())
                .unwrap_or_else(|| "The server closed the connection.".to_string()),
            Err(error) => describe_js_error(error),
        }
    }
}

struct DatagramWriter {
    writer: WritableStreamDefaultWriter,
}

impl DatagramWriter {
    async fn send(&mut self, message: ClientMessageUDP) -> Result<(), String> {
        let bytes = get_bincode_config().serialize(&message).map_err(|error| error.to_string())?;
        JsFuture::from(WritableStreamDefaultWriter::write_with_chunk(&self.writer, &Uint8Array::from(bytes.as_slice())))
            .await
            .map_err(describe_js_error)?;
        Ok(())
    }
}

// -------------------------- Reading the browser's streams --------------------------

// The next chunk a readable stream produced, or an error when the other side ended it.
async fn read_chunk(reader: &ReadableStreamDefaultReader) -> Result<Vec<u8>, String> {
    let read_result = JsFuture::from(reader.read()).await.map_err(describe_js_error)?;
    let is_done = Reflect::get(&read_result, &JsValue::from_str("done"))
        .map(|done| done.as_bool().unwrap_or(false))
        .unwrap_or(false);
    if is_done {
        return Err("The server closed the connection.".to_string());
    }
    let value = Reflect::get(&read_result, &JsValue::from_str("value")).map_err(describe_js_error)?;
    Ok(Uint8Array::new(&value).to_vec())
}

fn describe_js_error(error: JsValue) -> String {
    match error.dyn_ref::<js_sys::Error>() {
        Some(js_error) => js_error.message().into(),
        None => error.as_string().unwrap_or_else(|| "The game server could not be reached.".to_string()),
    }
}
