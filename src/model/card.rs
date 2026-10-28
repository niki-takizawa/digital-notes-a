use super::{AssetId, CardId, Evidence, SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CardEntityType {
    #[serde(rename = "card")]
    Card,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub schema_version: SchemaVersion,
    pub id: CardId,
    pub entity_type: CardEntityType,
    pub title: Title,
    pub created: UtcTimestamp,
    pub updated: UtcTimestamp,
    pub card_type: SnakeCase,
    pub created_by_work_id: WorkId,
    pub source_work_ids: Vec<WorkId>,
    pub source_asset_ids: Vec<AssetId>,
    pub source_card_ids: Vec<CardId>,
    pub evidence: Vec<Evidence>,
    pub body: String,
}
