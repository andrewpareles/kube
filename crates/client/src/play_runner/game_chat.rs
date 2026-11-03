use std::collections::VecDeque;

use crate::play_web_view::{HostToWebViewMessage, PlayWebView};
use serde::Serialize;
use shared::protocol::messages::{ClientGameChatMessage, GAME_CHAT_MAX_MESSAGE_CHARS, GameChatEntry};
use tokio::sync::{mpsc, watch};

// GameChat is a HUD overlay drawn by the play webview. Only messages echoed by the server enter the history.
// This owns the chat's end of the connection, and sends the page what it should show.
pub struct GameChat {
    message_tx: mpsc::Sender<ClientGameChatMessage>,
    history_rx: watch::Receiver<VecDeque<GameChatEntry>>,
    error: Option<&'static str>,
    sent_game_chat_state: Option<GameChatState>,
}

// Everything the page shows in the chat.
#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameChatState {
    entries: VecDeque<GameChatEntry>,
    error: Option<&'static str>,
    is_disconnected: bool,
    max_message_chars: usize,
}

impl GameChat {
    pub fn new(message_tx: mpsc::Sender<ClientGameChatMessage>, history_rx: watch::Receiver<VecDeque<GameChatEntry>>) -> Self {
        Self {
            message_tx,
            history_rx,
            error: None,
            sent_game_chat_state: None,
        }
    }

    // The page has already trimmed the message and checked it is not empty and has no control characters.
    pub fn send_message(&mut self, message: String) {
        self.error = match self.message_tx.try_send(ClientGameChatMessage { message }) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(_)) => Some("Chat is busy. Try again shortly."),
            Err(mpsc::error::TrySendError::Closed(_)) => Some("Chat disconnected."),
        };
    }

    pub fn send_to_web_view(&mut self, play_web_view: &mut PlayWebView) {
        let is_disconnected = self.message_tx.is_closed();
        let history_lock = self.history_rx.borrow();
        let is_page_current = self.sent_game_chat_state.as_ref().is_some_and(|sent_game_chat_state| {
            sent_game_chat_state.entries == *history_lock && sent_game_chat_state.error == self.error && sent_game_chat_state.is_disconnected == is_disconnected
        });
        if is_page_current {
            drop(history_lock);
            return;
        }
        let game_chat_state = GameChatState {
            entries: history_lock.clone(),
            error: self.error,
            is_disconnected,
            max_message_chars: GAME_CHAT_MAX_MESSAGE_CHARS,
        };
        drop(history_lock);
        play_web_view.send(&HostToWebViewMessage::SetGameChat(&game_chat_state));
        self.sent_game_chat_state = Some(game_chat_state);
    }
}
