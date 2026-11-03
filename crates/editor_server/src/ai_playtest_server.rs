use crate::playtest_process::{PlaytestLogSource, PlaytestProcess, PlaytestRequest};
use serde::de::DeserializeOwned;
use shared::api::protocol_for_chat::{ClientToolFuture, ClientToolTextFuture, HeadlessPlaytestHandler, ReadAiHeadlessPlaytestLogsArgs};
use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};

const PLAYTEST_READ_LOGS_DEFAULT_MAX_BYTES: usize = 16_000; // 16k of console output is a lot to read at once but still a small slice of a chat turn.
const MAX_PLAYTEST_LOG_ENTRIES: usize = 20_000; // 20k entries is a few minutes of a script logging every tick, and is dropped oldest-first so a chatty script cannot grow this forever.

// This is the top level struct, everything below is a helper
// This implements the handlers for the tool registry.
// (It needs to be a struct so we can implement the trait)
pub struct HeadlessPlaytestHandlerImpl;
impl HeadlessPlaytestHandler for HeadlessPlaytestHandlerImpl {
    fn start_server<'a>(&'a self, stored_world_string: String) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            let server_port = AI_PLAYTEST_SERVER.start_server(stored_world_string).await?;
            Ok(format!(
                "Started a test server on port {server_port} with no players on it yet.\nAdd players with start_ai_headless_playtest_client, then read what they print with read_ai_headless_playtest_logs starting at after_seq 0."
            ))
        })
    }

    fn start_client<'a>(&'a self) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            let client_id: u8 = AI_PLAYTEST_SERVER.request(PlaytestRequest::StartClient).await?;
            let source_text = PlaytestLogSource::Client { client_id }.to_text();
            Ok(format!(
                "{source_text} joined the game. Its console output shows up in read_ai_headless_playtest_logs under source \"{source_text}\", and stop_ai_headless_playtest_client takes client_id {client_id} to make it leave."
            ))
        })
    }

    fn stop_client<'a>(&'a self, client_id: u8) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            AI_PLAYTEST_SERVER.request::<()>(PlaytestRequest::StopClient { client_id }).await?;
            Ok(format!(
                "{} left the game. The server and the other players are still running.",
                PlaytestLogSource::Client { client_id }.to_text(),
            ))
        })
    }

    fn read_logs(&self, args: ReadAiHeadlessPlaytestLogsArgs) -> anyhow::Result<String> {
        let source = args
            .source
            .as_deref()
            .map(|source_text| {
                PlaytestLogSource::from_text(source_text)
                    .ok_or_else(|| anyhow::anyhow!("source must be \"server\" or \"client_<number>\", got \"{source_text}\""))
            })
            .transpose()?;
        let PlaytestLogReadout {
            entries,
            next_seq,
            has_more,
            dropped_entry_count,
        } = AI_PLAYTEST_SERVER.read_logs(
            args.after_seq.unwrap_or(0),
            args.max_bytes.unwrap_or(PLAYTEST_READ_LOGS_DEFAULT_MAX_BYTES),
            source,
        );

        // One entry can span several lines, so only its first line gets the source prefix and the rest line up under it
        let mut logs_text = String::new();
        for entry in entries.iter() {
            let entry_prefix = format!("[{} {}] ", entry.source.to_text(), entry.log_level);
            let entry_indent = " ".repeat(entry_prefix.len());
            let mut message_lines = entry.message.lines();
            let first_message_line = message_lines.next().unwrap_or("");
            logs_text.push_str(&format!("{entry_prefix}{first_message_line}\n"));
            for message_line in message_lines {
                logs_text.push_str(&format!("{entry_indent}{message_line}\n"));
            }
        }
        if entries.is_empty() {
            logs_text.push_str("(no new console output)\n");
        }
        if dropped_entry_count > 0 {
            logs_text.push_str(&format!("({dropped_entry_count} older entries were dropped before you read them)\n"));
        }
        if has_more {
            logs_text.push_str("(more output is waiting, read again from next_seq)\n");
        }
        logs_text.push_str(&format!("next_seq: {next_seq}"));
        Ok(logs_text)
    }

    fn capture_screenshot<'a>(&'a self, client_id: u8, width: u32, height: u32) -> ClientToolFuture<'a, Vec<u8>> {
        Box::pin(AI_PLAYTEST_SERVER.request(PlaytestRequest::CaptureScreenshot { client_id, width, height }))
    }

    fn stop_all(&self) -> String {
        if AI_PLAYTEST_SERVER.stop_all() {
            "Stopped the playtest.".to_string()
        } else {
            "no playtest was running.".to_string()
        }
    }
}

// ---------------------------------------------------- current playtest ----------------------------------------------------
struct PlaytestLogState {
    entries: VecDeque<PlaytestLogEntry>,
    next_seq: usize,
}

// Static object that the HeadlessPlaytestHandlerImpl uses (mostly private)
// Logs are cleared when a new playtest starts
pub static AI_PLAYTEST_SERVER: LazyLock<AiPlaytestServer> = LazyLock::new(|| AiPlaytestServer {
    running_playtest: StdMutex::new(None),
    logs: StdMutex::new(PlaytestLogState {
        entries: VecDeque::new(),
        next_seq: 0,
    }),
});
pub struct AiPlaytestServer {
    // A playtest that crashed stays here, answering every request with why it stopped, until the next one starts.
    running_playtest: StdMutex<Option<Arc<PlaytestProcess>>>,
    logs: StdMutex<PlaytestLogState>,
}
impl AiPlaytestServer {
    fn get_curr(&self) -> std::sync::MutexGuard<'_, Option<Arc<PlaytestProcess>>> {
        self.running_playtest.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn start_server(&self, stored_world_string: String) -> anyhow::Result<u16> {
        self.stop_all();
        self.clear_logs();
        let (process, server_port) = PlaytestProcess::start(&std::env::current_exe()?, stored_world_string, None, |source, log_level, message| {
            AI_PLAYTEST_SERVER.push_log_entry(source, log_level, message)
        })
        .await?;
        // Done this way so 2 items called at the same time work properly, apparently
        *self.get_curr() = Some(Arc::new(process));
        Ok(server_port)
    }

    async fn request<T: DeserializeOwned>(&self, request: PlaytestRequest) -> anyhow::Result<T> {
        let process_arc = self
            .get_curr()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no playtest is running. start one with start_ai_headless_playtest_server first."))?;
        process_arc.request(request).await
    }

    // Stop the server and every client, and say whether there was anything to stop. Safe to call when nothing is running, which is why the end of every AI turn calls it.
    pub fn stop_all(&self) -> bool {
        self.get_curr().take().is_some()
    }

    // ---------------------------- logs ----------------------------

    fn get_logs(&self) -> std::sync::MutexGuard<'_, PlaytestLogState> {
        self.logs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn clear_logs(&self) {
        let mut logs_lock = self.get_logs();
        logs_lock.entries.clear();
        logs_lock.next_seq = 0;
        drop(logs_lock);
    }

    fn push_log_entry(&self, source: PlaytestLogSource, log_level: String, message: String) {
        let mut logs_lock = self.get_logs();
        let seq = logs_lock.next_seq;
        logs_lock.next_seq += 1;
        logs_lock.entries.push_back(PlaytestLogEntry {
            seq,
            source,
            log_level,
            message,
        });
        if logs_lock.entries.len() > MAX_PLAYTEST_LOG_ENTRIES {
            logs_lock.entries.pop_front();
        }
        drop(logs_lock);
    }

    // Read log entries with seq >= after_seq, stopping once the messages add up to max_bytes.
    fn read_logs(&self, after_seq: usize, max_bytes: usize, source: Option<PlaytestLogSource>) -> PlaytestLogReadout {
        let logs_lock = self.get_logs();
        let oldest_kept_seq = logs_lock.entries.front().map(|entry| entry.seq).unwrap_or(logs_lock.next_seq);
        let dropped_entry_count = oldest_kept_seq.saturating_sub(after_seq);

        let mut entries = Vec::new();
        let mut total_bytes = 0;
        let mut has_more = false;
        for entry in logs_lock.entries.iter() {
            if entry.seq < after_seq {
                continue;
            }
            if let Some(source) = source
                && entry.source != source
            {
                continue;
            }
            total_bytes += entry.message.len();
            if total_bytes > max_bytes && !entries.is_empty() {
                has_more = true;
                break;
            }
            entries.push(entry.clone());
        }

        let next_seq = entries.last().map(|entry| entry.seq + 1).unwrap_or(logs_lock.next_seq);
        drop(logs_lock);

        PlaytestLogReadout {
            entries,
            next_seq,
            has_more,
            dropped_entry_count,
        }
    }
}

#[derive(Clone, Debug)]
struct PlaytestLogEntry {
    seq: usize, // Index in the entries, stored because we start forgetting old entries after a while
    source: PlaytestLogSource,
    log_level: String,
    message: String, // Can be several lines. 1 log entry.
}

// The readout datastructure
struct PlaytestLogReadout {
    entries: Vec<PlaytestLogEntry>,
    next_seq: usize,            // Pass this back as after_seq to continue where this page stopped
    has_more: bool,             // True when max_bytes cut the page short
    dropped_entry_count: usize, // Entries the caller asked for that had already been dropped from the front
}
