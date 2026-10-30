use crate::errors::FoguError;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Serialize, Deserialize)]
struct StoredValue {
    data: serde_json::Value,
    expires_at: Option<u64>,
}

pub struct KvStore {
    db: Db,
}

impl KvStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl: Option<u64>,
    ) -> Result<(), FoguError> {
        let stored = StoredValue {
            data: value,
            expires_at: ttl.map(|t| now_secs() + t),
        };
        self.db
            .insert(key.as_bytes(), serde_json::to_vec(&stored)?)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<serde_json::Value>, FoguError> {
        let Some(bytes) = self.db.get(key.as_bytes())? else {
            return Ok(None);
        };
        let stored: StoredValue = serde_json::from_slice(&bytes)?;
        if stored
            .expires_at
            .map_or(false, |exp| now_secs() > exp)
        {
            self.db.remove(key.as_bytes())?;
            return Ok(None);
        }
        Ok(Some(stored.data))
    }

    pub fn del(&self, key: &str) -> Result<bool, FoguError> {
        Ok(self.db.remove(key.as_bytes())?.is_some())
    }

    pub fn exists(&self, key: &str) -> Result<bool, FoguError> {
        Ok(self.get(key)?.is_some())
    }

    pub fn incr(&self, key: &str, delta: i64) -> Result<i64, FoguError> {
        let cur = match self.get(key)? {
            Some(v) => v
                .as_i64()
                .ok_or_else(|| FoguError::App("value is not an integer".into()))?,
            None => 0,
        };
        let new = cur + delta;
        self.set(key, serde_json::json!(new), None)?;
        Ok(new)
    }

    pub fn purge_expired(&self) -> usize {
        let now = now_secs();
        let mut n = 0;
        for item in self.db.iter() {
            let Ok((k, v)) = item else {
                continue;
            };
            if let Ok(s) = serde_json::from_slice::<StoredValue>(&v) {
                if s.expires_at.map_or(false, |exp| now > exp) {
                    self.db.remove(&k).ok();
                    n += 1;
                }
            }
        }
        n
    }
}

// Payloads
#[derive(Debug, Serialize, Deserialize)]
pub struct SetPayload {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPayload {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DelPayload {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExistsPayload {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IncrDecrPayload {
    pub key: String,
    #[serde(default = "default_delta")]
    pub delta: i64,
}

fn default_delta() -> i64 {
    1
}