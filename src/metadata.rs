use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const METADATA_FILE: &str = "_metadata.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub projects: BTreeMap<String, Project>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Project {
    pub api_key_sha256: String,
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub public: bool,
    pub size: u64,
    pub modified_at: DateTime<Utc>,
}

impl Metadata {
    pub async fn load(storage_path: &Path) -> Result<Self> {
        let path = storage_path.join(METADATA_FILE);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content)
                .with_context(|| format!("cannot parse {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                version: default_version(),
                projects: BTreeMap::new(),
            }),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub async fn save(&self, storage_path: &Path) -> Result<()> {
        let path = storage_path.join(METADATA_FILE);
        let tmp = storage_path.join(format!("{METADATA_FILE}.tmp"));
        let content = serde_json::to_vec_pretty(self).context("cannot serialize metadata")?;

        tokio::fs::write(&tmp, &content)
            .await
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .with_context(|| format!("cannot rename {} to {}", tmp.display(), path.display()))?;

        Ok(())
    }
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut out, "{b:02x}").expect("writing to a String cannot fail");
    }
    out
}
