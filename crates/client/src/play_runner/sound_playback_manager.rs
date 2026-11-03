use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween,
    sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings},
};
use game_types::{
    consts::JITTER_MARGIN_MS,
    data_types::{DataId, SoundData},
    object_types::{ClockInfo, ClockState, ObjectId, ObjectInfo, ObjectKind},
};
use shared::game::{data_fetcher::DataFetcherArc, world::World};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::{Arc, mpsc},
};

// Only seek when audio is farther from ClockInfo than the engine's existing clock jitter allowance.
const SOUND_CLOCK_RESYNC_THRESHOLD_SECS: f32 = JITTER_MARGIN_MS / 1000.0;

fn decibels_from_linear_volume(volume: f32) -> Decibels {
    if volume <= 0.0 {
        return Decibels::SILENCE;
    }
    Decibels(20.0 * volume.log10())
}

// Kira's decoder wants an owned AsRef<[u8]>; this borrows the Arc'd bytes instead of copying them.
struct SoundDataOggBytes(Arc<SoundData>);
impl AsRef<[u8]> for SoundDataOggBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0.oggVorbisData
    }
}

// Follow animation clock wrapping while keeping non-loop sounds silent before and after their range.
fn playback_time_secs_for_clock(clock_info: &ClockInfo, duration_secs: f32, est_dely_server_wall_ms: f64, live_client_wall_ms: f64) -> Option<f32> {
    let raw_time_secs = clock_info
        .clockState
        .readRawTimeMs(clock_info.speed, est_dely_server_wall_ms, Some(live_client_wall_ms))
        / 1000.0;
    let is_clock_outside_sound = if clock_info.speed.is_sign_negative() {
        raw_time_secs <= 0.0 || raw_time_secs > duration_secs as f64
    } else {
        raw_time_secs < 0.0 || raw_time_secs >= duration_secs as f64
    };
    if !clock_info.shouldLoop && is_clock_outside_sound {
        return None;
    }
    Some(clock_info.clockState.readTimeSecs(
        clock_info.speed,
        clock_info.shouldLoop,
        duration_secs,
        est_dely_server_wall_ms,
        Some(live_client_wall_ms),
    ))
}

// Playback requires a Clock that is playing at a finite, nonzero speed.
fn playing_clock_info_for_clock_id<'world>(world: &'world World, clock_id: &ObjectId) -> Option<&'world ClockInfo> {
    let clock_object = world.objectOfId.get(clock_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return None;
    };
    let ClockState::Playing { .. } = clock_info.clockState else {
        return None;
    };
    if !clock_info.speed.is_finite() || clock_info.speed.abs() <= f32::EPSILON {
        return None;
    }
    Some(clock_info)
}

struct SoundPlayback {
    sound_data_id: DataId,
    clock_id: ObjectId,
    clock_state: ClockState,
    should_loop: bool,
    speed: f32,
    volume: f32,
    duration_secs: f32,
    sound_handle: StaticSoundHandle,
}

impl Drop for SoundPlayback {
    fn drop(&mut self) {
        self.sound_handle.stop(Tween::default());
    }
}

// The sound playback manager turns replicated or local world clocks into client-only audio playback.
pub struct SoundPlaybackManager {
    // AudioManager.play(...) gives us a StaticSoundHandle
    soundPlaybackOfSoundPlayerId: HashMap<ObjectId, SoundPlayback>, // SoundPlayer id -> current playback
    decodedSoundDataOfDataId: HashMap<DataId, StaticSoundData>,     // data id -> decoded Kira sound data
    // The decode worker sends one result through tx; the play loop stores and polls this rx without blocking.
    pending_sound_data_decode: Option<(DataId, mpsc::Receiver<Result<StaticSoundData, String>>)>,

    sound_data_ids_with_decode_errors: HashSet<DataId>,
    audio_manager: AudioManager<DefaultBackend>,
    data_fetcher: DataFetcherArc,
}

impl SoundPlaybackManager {
    // None when there is no audio output device; the game then plays silently instead of failing.
    pub fn new(data_fetcher: DataFetcherArc) -> Option<Self> {
        let audio_manager = match AudioManager::new(AudioManagerSettings::default()) {
            Ok(audio_manager) => audio_manager,
            Err(error) => {
                log::warn!("No audio output device available, sounds will not play: {}", error);
                return None;
            }
        };

        Some(Self {
            soundPlaybackOfSoundPlayerId: HashMap::new(),
            decodedSoundDataOfDataId: HashMap::new(),
            pending_sound_data_decode: None,
            sound_data_ids_with_decode_errors: HashSet::new(),
            data_fetcher,
            audio_manager,
        })
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.audio_manager
            .main_track()
            .set_volume(decibels_from_linear_volume(volume), Tween::default());
    }

    // This updates client-only sound playback from the current SoundPlayerInfo and ClockInfo objects in World.
    // Kira advances audio independently; drift within the engine's jitter allowance (currently 50 ms) is ignored, while larger drift causes a seek.
    // Each call also accepts finished decode work, removes stale handles, starts ready sounds, and releases unused decoded audio.
    pub fn updateSoundPlaybackFromWorld(&mut self, world: &World, est_dely_server_wall_ms: f64, live_client_wall_ms: f64) {
        // First, poll the pending decode rx and move a finished result into the cache used to start Kira playback handles.
        let completed_sound_data_decode = match &self.pending_sound_data_decode {
            Some((sound_data_id, sound_data_decode_result_rx)) => match sound_data_decode_result_rx.try_recv() {
                Ok(result) => Some((*sound_data_id, result)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some((*sound_data_id, Err("Sound data decoder disconnected".to_string()))),
            },
            None => None,
        };
        if let Some((sound_data_id, decoded_sound_data_result)) = completed_sound_data_decode {
            self.pending_sound_data_decode = None;
            match decoded_sound_data_result {
                Ok(decoded_sound_data) => {
                    self.decodedSoundDataOfDataId.insert(sound_data_id, decoded_sound_data);
                }
                Err(error) => {
                    log::error!("Failed to decode SoundData {}: {}", sound_data_id, error);
                    self.sound_data_ids_with_decode_errors.insert(sound_data_id);
                }
            }
        }

        // Next, stop playback whose SoundPlayer object was removed from the World.
        self.soundPlaybackOfSoundPlayerId.retain(|sound_player_id, _| {
            world
                .objectOfId
                .get(sound_player_id)
                .is_some_and(|object| matches!(object.info, ObjectInfo::SoundPlayer(_)))
        });

        // Then, reconcile every SoundPlayer from the efficient reverse map instead of scanning the whole World.
        let sound_player_ids = world._kindOfObjectId.get_keys(&ObjectKind::SoundPlayer);
        let mut referenced_sound_data_ids = HashSet::new();
        for sound_player_id in sound_player_ids.into_iter().flatten() {
            let Some(sound_player_object) = world.objectOfId.get(sound_player_id) else {
                continue;
            };
            let ObjectInfo::SoundPlayer(sound_player_info) = &sound_player_object.info else {
                continue;
            };
            let sound_data_id = sound_player_info.soundDataId;
            referenced_sound_data_ids.insert(sound_data_id);

            let usable_clock_info = sound_player_info
                .clockId
                .map(Into::into)
                .and_then(|clock_id: ObjectId| playing_clock_info_for_clock_id(world, &clock_id).map(|clock_info| (clock_id, clock_info)));
            let Some((clock_id, clock_info)) = usable_clock_info else {
                self.soundPlaybackOfSoundPlayerId.remove(sound_player_id);
                continue;
            };
            if !sound_player_info.volume.is_finite() {
                self.soundPlaybackOfSoundPlayerId.remove(sound_player_id);
                continue;
            }

            // Source or timeline changes need a new Kira handle; volume and clock drift can use the current handle.
            let reusable_sound_playback = self.soundPlaybackOfSoundPlayerId.get_mut(sound_player_id).filter(|sound_playback| {
                sound_playback.sound_data_id == sound_data_id
                    && sound_playback.clock_id == clock_id
                    && sound_playback.clock_state == clock_info.clockState
                    && sound_playback.should_loop == clock_info.shouldLoop
                    && sound_playback.speed == clock_info.speed
            });

            // A matching handle runs on Kira's audio thread. This block only updates volume and corrects meaningful clock drift.
            if let Some(sound_playback) = reusable_sound_playback {
                let duration_secs = sound_playback.duration_secs;
                let Some(clock_playback_time_secs) = playback_time_secs_for_clock(clock_info, duration_secs, est_dely_server_wall_ms, live_client_wall_ms)
                else {
                    self.soundPlaybackOfSoundPlayerId.remove(sound_player_id);
                    continue;
                };
                let is_playback_advancing = sound_playback.sound_handle.state().is_advancing();
                let playback_position_secs = sound_playback.sound_handle.position() as f32;

                // Looping sounds use the shorter distance across the loop boundary when measuring drift.
                let playback_position_error_secs = if clock_info.shouldLoop {
                    let direct_playback_position_error_secs = (playback_position_secs.rem_euclid(duration_secs) - clock_playback_time_secs).abs();
                    direct_playback_position_error_secs.min(duration_secs - direct_playback_position_error_secs)
                } else {
                    (playback_position_secs - clock_playback_time_secs).abs()
                };
                let clock_time_error_secs = playback_position_error_secs / clock_info.speed.abs();
                if is_playback_advancing {
                    if sound_playback.volume != sound_player_info.volume {
                        sound_playback.volume = sound_player_info.volume;
                        sound_playback
                            .sound_handle
                            .set_volume(decibels_from_linear_volume(sound_player_info.volume), Tween::default());
                    }
                    if clock_time_error_secs > SOUND_CLOCK_RESYNC_THRESHOLD_SECS {
                        sound_playback.sound_handle.seek_to(clock_playback_time_secs as f64);
                    }
                    continue;
                }

                // A finished non-looping sound stays silent. Any other stopped handle falls through to be replaced below.
                if !clock_info.shouldLoop && clock_time_error_secs <= SOUND_CLOCK_RESYNC_THRESHOLD_SECS {
                    continue;
                }
            }
            // Dropping a stale or stopped entry stops its handle before a replacement starts below.
            self.soundPlaybackOfSoundPlayerId.remove(sound_player_id);

            // No usable handle remains. Fetch compressed data as needed and run at most one background decode at a time.
            if self.sound_data_ids_with_decode_errors.contains(&sound_data_id) {
                continue;
            }
            let Some(decoded_sound_data) = self.decodedSoundDataOfDataId.get(&sound_data_id) else {
                let Some(sound_data_arc) = self.data_fetcher.getOrFetchSoundData(sound_data_id) else {
                    continue;
                };
                if self.pending_sound_data_decode.is_some() {
                    continue;
                }

                // Keep one decode in flight so large user sounds cannot start many CPU-heavy workers at once.
                let (sound_data_decode_result_tx, sound_data_decode_result_rx) = mpsc::sync_channel(1);
                self.pending_sound_data_decode = Some((sound_data_id, sound_data_decode_result_rx));
                std::thread::spawn(move || {
                    let sound_data_decode_result =
                        StaticSoundData::from_cursor(Cursor::new(SoundDataOggBytes(sound_data_arc))).map_err(|error| error.to_string());
                    let _ = sound_data_decode_result_tx.send(sound_data_decode_result);
                });
                continue;
            };
            let duration_secs = decoded_sound_data.duration().as_secs_f32();
            if duration_secs <= f32::EPSILON {
                continue;
            }
            let Some(clock_playback_time_secs) = playback_time_secs_for_clock(clock_info, duration_secs, est_dely_server_wall_ms, live_client_wall_ms) else {
                continue;
            };

            // Share decoded frames, but give each SoundPlayer its own settings and handle at the Clock's current position.
            let mut sound_settings = StaticSoundSettings::new()
                .start_position(clock_playback_time_secs as f64)
                .volume(decibels_from_linear_volume(sound_player_info.volume))
                .playback_rate(clock_info.speed as f64);
            if clock_info.shouldLoop {
                sound_settings = sound_settings.loop_region(..);
            }
            let sound_data_with_settings = decoded_sound_data.with_settings(sound_settings);
            let sound_handle = match self.audio_manager.play(sound_data_with_settings) {
                Ok(sound_handle) => sound_handle,
                Err(error) => {
                    log::error!("Failed to play SoundPlayer {}: {}", sound_player_id, error);
                    continue;
                }
            };
            self.soundPlaybackOfSoundPlayerId.insert(
                *sound_player_id,
                SoundPlayback {
                    sound_data_id,
                    clock_id,
                    clock_state: clock_info.clockState,
                    should_loop: clock_info.shouldLoop,
                    speed: clock_info.speed,
                    volume: sound_player_info.volume,
                    duration_secs,
                    sound_handle,
                },
            );
        }

        // Finally, keep decoded PCM only while at least one SoundPlayer still references its immutable DataId.
        self.decodedSoundDataOfDataId
            .retain(|sound_data_id, _| referenced_sound_data_ids.contains(sound_data_id));
    }
}
