use crate::{errors::FoguError, permissions::ApiKeyPerms};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub token: String,
    pub name: String,
    pub perms: ApiKeyPerms,
    pub created_at: u64,
}

impl ApiKey {
    pub fn new(name: String, perms: ApiKeyPerms) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            token: format!("fk_{}", Uuid::new_v4().simple()),
            name,
            perms,
            created_at: now_secs(),
        }
    }
}

/// Persistent API key store backed by sled (separate tree).
pub struct ApiKeyStore {
    tree: sled::Tree,
}

impl ApiKeyStore {
    const PREFIX: &'static str = "__apikey__:";

    pub fn new(db: &Db) -> Result<Self, FoguError> {
        Ok(Self {
            tree: db.open_tree("_fogu_apikeys")?,
        })
    }

    pub fn insert(&self, key: &ApiKey) -> Result<(), FoguError> {
        self.tree.insert(
            format!("{}{}", Self::PREFIX, key.id).as_bytes(),
            serde_json::to_vec(key)?,
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<bool, FoguError> {
        Ok(self
            .tree
            .remove(format!("{}{}", Self::PREFIX, id).as_bytes())?
            .is_some())
    }

    pub fn list(&self) -> Result<Vec<ApiKey>, FoguError> {
        let mut result = Vec::new();
        for item in self.tree.iter() {
            let (_, v) = item?;
            if let Ok(k) = serde_json::from_slice::<ApiKey>(&v) {
                result.push(k);
            }
        }
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(result)
    }

    pub fn find_by_token(&self, token: &str) -> Result<Option<ApiKey>, FoguError> {
        for item in self.tree.iter() {
            let (_, v) = item?;
            if let Ok(k) = serde_json::from_slice::<ApiKey>(&v) {
                if k.token == token {
                    return Ok(Some(k));
                }
            }
        }
        Ok(None)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<ApiKey>, FoguError> {
        let bytes = self
            .tree
            .get(format!("{}{}", Self::PREFIX, id).as_bytes())?;
        Ok(bytes.and_then(|v| serde_json::from_slice(&v).ok()))
    }
}

/// Public view of an API key (token only shown on create).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyView {
    pub id: String,
    pub name: String,
    pub perms: ApiKeyPerms,
    pub created_at: u64,
    /// Present only when first created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl From<&ApiKey> for ApiKeyView {
    fn from(k: &ApiKey) -> Self {
        Self {
            id: k.id.clone(),
            name: k.name.clone(),
            perms: k.perms.clone(),
            created_at: k.created_at,
            token: None,
        }
    }
}

impl ApiKeyView {
    pub fn with_token(k: &ApiKey) -> Self {
        let mut v = ApiKeyView::from(k);
        v.token = Some(k.token.clone());
        v
    }
}

// API Key management payloads
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyCreatePayload {
    pub name: String,
    /// Permissions granted to this key.
    pub channels: Vec<String>,
    pub kv: Vec<String>,
    pub topics: Vec<String>,
    pub rpc: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyDeletePayload {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyGetPayload {
    pub id: String,
}