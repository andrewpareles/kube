use game_types::GameId;
use game_types::data_types::{DataId, DataKind};
use render_core::dehydrated_gltf::BlenderId;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

// Every file we keep lives in one of two S3 buckets. Public files are served to anyone by CloudFront at cdn.kube.new, and private
// files are read only by our servers. A file named by an integer id is found from the id alone; a file named by a uuid is found
// through the row that points at it. Every file is written once and never changed, so a cached copy is never out of date.
// Text and meshes are stored gzipped, and readers unzip them on the way in. Files that are already compressed (PNG, Ogg, KTX2) or
// that are read in byte ranges (textures, sounds) are stored as they are.
pub const PUBLIC_BUCKET: &str = "kube-new-public";
pub const PRIVATE_BUCKET: &str = "kube-new-private";
pub const CDN_ORIGIN: &str = "https://cdn.kube.new";

// -------------------------- Public files --------------------------

pub enum PublicFile {
    // Gzipped for meshes only. Textures and sounds are stored as they are.
    Data { data_kind: DataKind, data_id: DataId },
    // Gzipped.
    Catalog { catalog_item_id: i64 },
    // Not gzipped.
    GameThumbnail { thumbnail_uuid: Uuid },
    // Gzipped.
    Asset { asset_uuid: Uuid },
}

impl PublicFile {
    pub fn key(&self) -> String {
        match self {
            Self::Data { data_kind, data_id } => format!("data/{}/{data_id}", data_kind.supabase_data_type_str()),
            Self::Catalog { catalog_item_id } => format!("catalog/{catalog_item_id}"),
            Self::GameThumbnail { thumbnail_uuid } => format!("game-thumbnails/{thumbnail_uuid}"),
            Self::Asset { asset_uuid } => format!("assets/{asset_uuid}"),
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Data { .. } => "application/octet-stream",
            Self::Catalog { .. } | Self::Asset { .. } => "application/json",
            Self::GameThumbnail { .. } => "image/png",
        }
    }

    pub fn is_gzipped(&self) -> bool {
        match self {
            Self::Data { data_kind, .. } => matches!(data_kind, DataKind::MeshData),
            Self::Catalog { .. } | Self::Asset { .. } => true,
            Self::GameThumbnail { .. } => false,
        }
    }

    pub fn url(&self) -> String {
        format!("{CDN_ORIGIN}/{}", self.key())
    }
}

// -------------------------- Private files --------------------------

pub enum PrivateFile {
    // Gzipped.
    GameWorld { world_uuid: Uuid },
    // Gzipped.
    EditorWorld { world_uuid: Uuid },
    // Not gzipped.
    ChatScreenshot { game_id: GameId, screenshot_uuid: Uuid },
    // Not gzipped. Whether it is a .blend or a .glb is in its blender_metadata row.
    Blender { blender_id: BlenderId },
}

impl PrivateFile {
    pub fn key(&self) -> String {
        match self {
            Self::GameWorld { world_uuid } => format!("game-worlds/{world_uuid}"),
            Self::EditorWorld { world_uuid } => format!("editor-worlds/{world_uuid}"),
            Self::ChatScreenshot { game_id, screenshot_uuid } => format!("chat-screenshots/{game_id}/{screenshot_uuid}"),
            Self::Blender { blender_id } => format!("blender/{blender_id}"),
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            Self::GameWorld { .. } | Self::EditorWorld { .. } => "application/json",
            Self::ChatScreenshot { .. } => "image/png",
            Self::Blender { .. } => "application/octet-stream",
        }
    }

    pub fn is_gzipped(&self) -> bool {
        match self {
            Self::GameWorld { .. } | Self::EditorWorld { .. } => true,
            Self::ChatScreenshot { .. } | Self::Blender { .. } => false,
        }
    }
}

// -------------------------- Reading and writing --------------------------

// Only a few downloads run at once: a player joining can ask for hundreds of files, and a connection for each one runs out of open
// files on a Mac (apps start with a limit of 256). A download that stops getting bytes gives up, so it can't keep its turn forever.
// TODO: AI-PICKED-VALUE: 8 downloads at once, a 10 second connect timeout, and a 30 second wait for the next bytes.
static CDN_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("CDN HTTP client")
});
static CDN_DOWNLOAD_PERMITS: Semaphore = Semaphore::const_new(8);
// TODO: AI-PICKED-VALUE: A failed download is tried again after 1, 2 and 4 seconds, so one network hiccup doesn't leave a file missing
// for the whole game. These are the same waits as the editor's credit reads.
const CDN_DOWNLOAD_RETRY_DELAYS_MS: [u64; 3] = [1000, 2000, 4000];

// Public files are read through the CDN, the same way players read them, and reqwest unzips a gzipped response by itself.
pub async fn download_public_file(file: &PublicFile) -> anyhow::Result<Vec<u8>> {
    async fn download_once(url: &str) -> reqwest::Result<Vec<u8>> {
        let _cdn_download_permit = CDN_DOWNLOAD_PERMITS.acquire().await.expect("CDN download permits are never closed");
        Ok(CDN_CLIENT.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
    }
    let url = file.url();
    for delay_ms in CDN_DOWNLOAD_RETRY_DELAYS_MS {
        let error = match download_once(&url).await {
            Ok(bytes) => return Ok(bytes),
            Err(error) => error,
        };
        // A missing file stays missing.
        if error.status().is_some_and(|status| status.is_client_error()) {
            return Err(error.into());
        }
        log::warn!("Could not download {url}, trying again in {delay_ms}ms: {:#}", anyhow::Error::from(error));
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    Ok(download_once(&url).await?)
}

// Writing files and reading private ones need our AWS key and the AWS SDK. Only the editor server and the game server turn on the s3
// feature, so the desktop app never builds the SDK.
#[cfg(feature = "s3")]
pub use with_aws_key::{download_private_file, upload_private_file, upload_public_file};

#[cfg(feature = "s3")]
mod with_aws_key {
    use super::{PRIVATE_BUCKET, PUBLIC_BUCKET, PrivateFile, PublicFile};
    use crate::env::env_vars::{S3_ACCESS_KEY_ID, S3_SECRET_ACCESS_KEY};
    use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
    use aws_sdk_s3::error::DisplayErrorContext;
    use aws_sdk_s3::primitives::ByteStream;
    use flate2::{Compression, read::GzDecoder, write::GzEncoder};
    use std::io::{Read, Write};
    use std::sync::LazyLock;

    const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

    static S3_CLIENT: LazyLock<aws_sdk_s3::Client> = LazyLock::new(|| {
        let credentials = Credentials::new(S3_ACCESS_KEY_ID.as_str(), S3_SECRET_ACCESS_KEY.as_str(), None, None, "kube-new");
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(credentials)
            .build();
        aws_sdk_s3::Client::from_conf(config)
    });

    // An upload only ever creates a file. It gzips the file if its kind says so, sends the immutable cache header, and S3 refuses it
    // if the key already exists.
    pub async fn upload_public_file(file: &PublicFile, bytes: Vec<u8>) -> anyhow::Result<()> {
        upload(PUBLIC_BUCKET, &file.key(), file.content_type(), file.is_gzipped(), bytes).await
    }

    pub async fn upload_private_file(file: &PrivateFile, bytes: Vec<u8>) -> anyhow::Result<()> {
        upload(PRIVATE_BUCKET, &file.key(), file.content_type(), file.is_gzipped(), bytes).await
    }

    // S3 returns a file exactly as it was stored, so a gzipped one is unzipped here.
    pub async fn download_private_file(file: &PrivateFile) -> anyhow::Result<Vec<u8>> {
        let key = file.key();
        let object = S3_CLIENT
            .get_object()
            .bucket(PRIVATE_BUCKET)
            .key(&key)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("Failed to download {PRIVATE_BUCKET}/{key}: {}", DisplayErrorContext(error)))?;
        let is_gzipped = object.content_encoding() == Some("gzip");
        let bytes = object.body.collect().await?.into_bytes();
        if !is_gzipped {
            return Ok(bytes.to_vec());
        }
        let mut unzipped = Vec::new();
        GzDecoder::new(&bytes[..]).read_to_end(&mut unzipped)?;
        Ok(unzipped)
    }

    async fn upload(bucket_name: &'static str, key: &str, content_type: &str, is_gzipped: bool, bytes: Vec<u8>) -> anyhow::Result<()> {
        let body = if is_gzipped { gzip(&bytes)? } else { bytes };
        S3_CLIENT
            .put_object()
            .bucket(bucket_name)
            .key(key)
            .content_type(content_type)
            .cache_control(IMMUTABLE_CACHE_CONTROL)
            .if_none_match("*")
            .set_content_encoding(is_gzipped.then(|| "gzip".to_string()))
            .body(ByteStream::from(body))
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("Failed to upload {bucket_name}/{key}: {}", DisplayErrorContext(error)))?;
        Ok(())
    }

    fn gzip(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        Ok(encoder.finish()?)
    }
}
