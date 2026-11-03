use std::collections::VecDeque;

use quinn::Connection;
use shared::protocol::{
    messages::{ClientGameChatMessage, ClientMessageTCP, GAME_CHAT_HISTORY_LIMIT, GameChatEntry, ServerGameChatMessage, ServerGameChatResultMessage},
    quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitTcpMessageMaxBytes, sendTcpMessage, sendTcpRequest},
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

// GameChat owns its stream and channels. Failures close GameChat; session shutdown only cancels this worker.
pub fn start_receiving_game_chat_messages_tcp_in_bg(
    connection: Connection,
    kill_play_cancel_token: CancellationToken,
) -> (mpsc::Sender<ClientGameChatMessage>, watch::Receiver<VecDeque<GameChatEntry>>) {
    let (game_chat_history_tx, game_chat_history_rx) = watch::channel(VecDeque::new());
    // TODO: AI-PICKED-VALUE: Bound queued outgoing chat to eight messages while the connection is busy.
    let (game_chat_message_tx, mut game_chat_message_rx) = mpsc::channel::<ClientGameChatMessage>(8);

    tokio::spawn(async move {
        let run_game_chat = async {
            let (mut game_chat_send_stream, mut game_chat_recv_stream) = connection.open_bi().await?;
            sendTcpMessage(&mut game_chat_send_stream, &ClientMessageTCP::SubscribeToGameChat).await?;
            game_chat_send_stream.finish()?;

            // Forward messages from the chat input to the server.
            let send_messages = async {
                while let Some(message) = game_chat_message_rx.recv().await {
                    // Send one request at a time so submissions stay ordered across their separate streams.
                    let result = sendTcpRequest::<_, ServerGameChatResultMessage>(&connection, &ClientMessageTCP::GameChat(message))
                        .await
                        .and_then(|response| response.result.map_err(anyhow::Error::msg));
                    if let Err(error) = result {
                        log::warn!("Could not send chat message: {}", error);
                    }
                }
                Ok::<(), anyhow::Error>(())
            };

            // Keep a bounded client history even when no GUI frame is being rendered.
            // Only cancel a framed read when the entire chat connection is ending.
            let receive_messages = async {
                loop {
                    let message = match awaitTcpMessageMaxBytes::<ServerGameChatMessage>(&mut game_chat_recv_stream, MAX_TCP_MESSAGE_BYTES).await {
                        Ok(message) => message,
                        Err(error) => break Err::<(), _>(error),
                    };
                    game_chat_history_tx.send_modify(|history| match message {
                        ServerGameChatMessage::History(entries) => *history = entries,
                        ServerGameChatMessage::Entry(entry) => {
                            if history.len() == GAME_CHAT_HISTORY_LIMIT {
                                history.pop_front();
                            }
                            history.push_back(entry);
                        }
                    });
                }
            };

            // Headless playtest clients have no chat input, but still keep their stream open.
            // Joining both loops lets receiving continue after the input queue closes. Reads are
            // only cancelled when the whole task ends, so outgoing messages cannot interrupt a partial read.
            tokio::try_join!(send_messages, receive_messages)?;
            Ok::<(), anyhow::Error>(())
        };
        let result = tokio::select! {
            _ = kill_play_cancel_token.cancelled() => return,
            result = run_game_chat => result,
        };
        if let Err(error) = result {
            log::warn!("Chat connection interrupted: {}", error);
        }
    });

    (game_chat_message_tx, game_chat_history_rx)
}
