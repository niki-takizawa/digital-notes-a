use super::{SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkEntityType {
    #[serde(rename = "work")]
    Work,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Active,
    Done,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub schema_version: SchemaVersion,
    pub id: WorkId,
    pub entity_type: WorkEntityType,
    pub title: Title,
    pub created: UtcTimestamp,
    pub updated: UtcTimestamp,
    pub work_type: SnakeCase,
    pub status: WorkStatus,
    pub source_work_ids: Vec<WorkId>,
    pub body: String,
}
