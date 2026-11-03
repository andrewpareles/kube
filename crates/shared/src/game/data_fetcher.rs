use std::collections::{HashMap, HashSet};
use std::sync::Arc;
// TODO: how would we use tokio mpsc here?

use crate::game::uploaded_data::parse_uploaded_data_from_bytes;
use crate::helpers::multi_thread_hashmap::MultiThreadHashMap;
use crate::s3_storage::{PublicFile, download_public_file};
use futures::stream::{self, StreamExt};
use game_types::consts::{MESH_DATA_CURRENT_VERSION, SOUND_DATA_CURRENT_VERSION, TEXTURE_DATA_CURRENT_VERSION};
use game_types::data_types::{DataId, DataIds, DataKind, MeshData, SoundData, TextureData};
use tokio::sync::Mutex;

// TODO: AI-PICKED-VALUE: Preloads download 8 files at a time. Each download opens its own connection, and a world can preload
// thousands of files.
const PRELOAD_DOWNLOADS_AT_ONCE: usize = 8;

// ------------- DataFetcher -------------
// clone this anywhere, it's fine and cheap and shares the same underlying data
#[derive(Clone)]
pub struct DataFetcherArc {
    meshDataInfoOfDataIdArc: Arc<MultiThreadHashMap<DataId, MeshData>>,
    textureDataInfoOfDataIdArc: Arc<MultiThreadHashMap<DataId, TextureData>>,
    soundDataOfDataIdArc: Arc<MultiThreadHashMap<DataId, SoundData>>,

    pendingDataIdsArc: Arc<Mutex<HashSet<(DataKind, DataId)>>>, // include DataKind so probing one data_id as the wrong asset kind does not block the correct kind
    erroredDataIdsArc: Arc<Mutex<HashSet<(DataKind, DataId)>>>,
}
// the job of this file is to fetch dataWrapper, parse it to the most recent version, and return the data
impl DataFetcherArc {
    pub fn new() -> Self {
        Self {
            meshDataInfoOfDataIdArc: Arc::new(MultiThreadHashMap::new()),
            textureDataInfoOfDataIdArc: Arc::new(MultiThreadHashMap::new()),
            soundDataOfDataIdArc: Arc::new(MultiThreadHashMap::new()),

            pendingDataIdsArc: Arc::new(Mutex::new(HashSet::new())),
            erroredDataIdsArc: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    //  ----- Helpers -----
    // This starts a background fetch and uses the pending set to avoid duplicate work when the lock is available.
    fn fetchInBgIfNotPending<T, F, Fut>(
        data_map_arc: Arc<MultiThreadHashMap<DataId, T>>, // mutates this
        pending_data_ids_arc: Arc<Mutex<HashSet<(DataKind, DataId)>>>,
        errored_data_ids_arc: Arc<Mutex<HashSet<(DataKind, DataId)>>>,
        download_fn: F,
        data_kind: DataKind,
        data_id: DataId,
    ) where
        T: Send + Sync + 'static,
        F: FnOnce(DataId) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    {
        // preemptive heuristic to avoid spawning a task that doesn't need to run
        if let Ok(pending_data_ids_lock) = pending_data_ids_arc.try_lock() {
            if pending_data_ids_lock.contains(&(data_kind, data_id)) {
                return;
            }
        }
        if let Ok(errored_data_ids_lock) = errored_data_ids_arc.try_lock() {
            if errored_data_ids_lock.contains(&(data_kind, data_id)) {
                return;
            }
        }

        tokio::spawn(async move {
            // check for real to see if shouldn't run
            let pending_data_key = (data_kind, data_id);
            let mut pending_data_ids_lock = pending_data_ids_arc.lock().await;
            if pending_data_ids_lock.contains(&pending_data_key) {
                return;
            }
            pending_data_ids_lock.insert(pending_data_key);
            drop(pending_data_ids_lock);
            if errored_data_ids_arc.lock().await.contains(&pending_data_key) {
                pending_data_ids_arc.lock().await.remove(&pending_data_key);
                return;
            }

            match download_fn(data_id).await {
                Ok(data) => {
                    data_map_arc.insert(data_id, data);
                    errored_data_ids_arc.lock().await.remove(&pending_data_key);
                }
                Err(error) => {
                    log::error!("Failed to cache DataId {data_id}: {error}");
                    errored_data_ids_arc.lock().await.insert(pending_data_key);
                }
            }
            pending_data_ids_arc.lock().await.remove(&pending_data_key);
        });
    }

    //  ----- Get -----
    // one core assumption here is if get didn't work, you will simply try again, so try will eventually succeed

    pub fn getOrFetchMeshData(&self, data_id: DataId) -> Option<Arc<MeshData>> {
        let existing_data = self.meshDataInfoOfDataIdArc.get(&data_id);
        if existing_data.is_some() {
            return existing_data;
        } else {
            self.fetchMeshDataIfNeeded(data_id);
            return None;
        }
    }

    pub fn getOrFetchTextureData(&self, data_id: DataId) -> Option<Arc<TextureData>> {
        let existing_data = self.textureDataInfoOfDataIdArc.get(&data_id);
        if existing_data.is_some() {
            return existing_data;
        } else {
            self.fetchTextureDataIfNeeded(data_id);
            return None;
        }
    }

    pub fn getOrFetchSoundData(&self, data_id: DataId) -> Option<Arc<SoundData>> {
        let existing_data = self.soundDataOfDataIdArc.get(&data_id);
        if existing_data.is_some() {
            return existing_data;
        } else {
            self.fetchSoundDataIfNeeded(data_id);
            return None;
        }
    }

    // These never start a download.
    pub fn getDownloadedMeshData(&self, data_id: DataId) -> Option<Arc<MeshData>> {
        self.meshDataInfoOfDataIdArc.get(&data_id)
    }

    pub fn getDownloadedTextureData(&self, data_id: DataId) -> Option<Arc<TextureData>> {
        self.textureDataInfoOfDataIdArc.get(&data_id)
    }

    //  ----- Preload -----

    // Downloads data before anything needs it. Only a few download at once, so data that's needed right now doesn't wait behind them.
    pub async fn preloadDataIds(&self, data_ids: DataIds) {
        let DataIds {
            mesh_data_ids,
            texture_data_ids,
            sound_data_ids,
        } = data_ids;
        let data_keys = mesh_data_ids
            .into_iter()
            .map(|data_id| (DataKind::MeshData, data_id))
            .chain(texture_data_ids.into_iter().map(|data_id| (DataKind::TextureData, data_id)))
            .chain(sound_data_ids.into_iter().map(|data_id| (DataKind::SoundData, data_id)));
        stream::iter(data_keys)
            .for_each_concurrent(PRELOAD_DOWNLOADS_AT_ONCE, |data_key| self.waitForData(data_key))
            .await;
    }

    // Starts the download if it hasn't started, then waits until it has landed or failed.
    async fn waitForData(&self, (data_kind, data_id): (DataKind, DataId)) {
        loop {
            let is_downloaded = match data_kind {
                DataKind::MeshData => self.getOrFetchMeshData(data_id).is_some(),
                DataKind::TextureData => self.getOrFetchTextureData(data_id).is_some(),
                DataKind::SoundData => self.getOrFetchSoundData(data_id).is_some(),
            };
            if is_downloaded || self.erroredDataIdsArc.lock().await.contains(&(data_kind, data_id)) {
                return;
            }
            // TODO: AI-PICKED-VALUE: Check for finished downloads every 50ms, the same as waitForMeshAndTextureData.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    //  ----- Wait -----

    // Starts every download at once, then waits until each has landed or failed. Failed ones are left out.
    pub async fn waitForMeshAndTextureData(
        &self,
        mesh_data_ids: HashSet<DataId>,
        texture_data_ids: HashSet<DataId>,
    ) -> (HashMap<DataId, Arc<MeshData>>, HashMap<DataId, Arc<TextureData>>) {
        loop {
            let mesh_data_arc_of_id: HashMap<_, _> = mesh_data_ids.iter().filter_map(|id| Some((*id, self.getOrFetchMeshData(*id)?))).collect();
            let texture_data_arc_of_id: HashMap<_, _> = texture_data_ids.iter().filter_map(|id| Some((*id, self.getOrFetchTextureData(*id)?))).collect();
            let errored_data_ids_lock = self.erroredDataIdsArc.lock().await;
            let is_done = mesh_data_ids
                .iter()
                .all(|id| mesh_data_arc_of_id.contains_key(id) || errored_data_ids_lock.contains(&(DataKind::MeshData, *id)))
                && texture_data_ids
                    .iter()
                    .all(|id| texture_data_arc_of_id.contains_key(id) || errored_data_ids_lock.contains(&(DataKind::TextureData, *id)));
            drop(errored_data_ids_lock);
            if is_done {
                return (mesh_data_arc_of_id, texture_data_arc_of_id);
            }
            // TODO: AI-PICKED-VALUE: Check for finished downloads every 50ms.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    //  ----- Fetch if needed -----

    fn fetchMeshDataIfNeeded(&self, data_id: DataId) {
        async fn downloadMeshData(data_id: DataId) -> anyhow::Result<MeshData> {
            let data_kind = DataKind::MeshData;
            let bytes = download_public_file(&PublicFile::Data { data_kind, data_id }).await?;
            parse_uploaded_data_from_bytes(&bytes, MESH_DATA_CURRENT_VERSION)
        }
        let data_kind = DataKind::MeshData;
        Self::fetchInBgIfNotPending(
            self.meshDataInfoOfDataIdArc.clone(),
            self.pendingDataIdsArc.clone(),
            self.erroredDataIdsArc.clone(),
            downloadMeshData,
            data_kind,
            data_id,
        );
    }

    fn fetchTextureDataIfNeeded(&self, data_id: DataId) {
        async fn downloadTextureData(data_id: DataId) -> anyhow::Result<TextureData> {
            let data_kind = DataKind::TextureData;
            let bytes = download_public_file(&PublicFile::Data { data_kind, data_id }).await?;
            parse_uploaded_data_from_bytes(&bytes, TEXTURE_DATA_CURRENT_VERSION)
        }
        let data_kind = DataKind::TextureData;
        Self::fetchInBgIfNotPending(
            self.textureDataInfoOfDataIdArc.clone(),
            self.pendingDataIdsArc.clone(),
            self.erroredDataIdsArc.clone(),
            downloadTextureData,
            data_kind,
            data_id,
        );
    }

    fn fetchSoundDataIfNeeded(&self, data_id: DataId) {
        async fn downloadSoundData(data_id: DataId) -> anyhow::Result<SoundData> {
            let data_kind = DataKind::SoundData;
            let bytes = download_public_file(&PublicFile::Data { data_kind, data_id }).await?;
            parse_uploaded_data_from_bytes(&bytes, SOUND_DATA_CURRENT_VERSION)
        }
        let data_kind = DataKind::SoundData;
        Self::fetchInBgIfNotPending(
            self.soundDataOfDataIdArc.clone(),
            self.pendingDataIdsArc.clone(),
            self.erroredDataIdsArc.clone(),
            downloadSoundData,
            data_kind,
            data_id,
        );
    }
}
