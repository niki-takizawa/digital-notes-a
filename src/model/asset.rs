use super::{AssetId, ContentHash, HttpUrl, SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AssetEntityType {
    #[serde(rename = "asset")]
    Asset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HtmlFileFormat {
    #[serde(rename = "html")]
    Html,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    #[serde(rename = "sha256")]
    Sha256,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub schema_version: SchemaVersion,
    pub id: AssetId,
    pub entity_type: AssetEntityType,
    pub title: Title,
    pub created: UtcTimestamp,
    pub updated: UtcTimestamp,
    pub asset_type: SnakeCase,
    pub url: HttpUrl,
    pub file_format: HtmlFileFormat,
    pub capture_method: SnakeCase,
    pub created_by_work_id: WorkId,
    pub related_work_ids: Vec<WorkId>,
    pub content_hash: ContentHash,
    pub hash_algorithm: HashAlgorithm,
}
