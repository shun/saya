use crate::presentation_effect::OverlayContentKey;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMediaType {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayAssetRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayAssetMetadata {
    pub media_type: OverlayMediaType,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub alt_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayAssetMedia {
    pub metadata: OverlayAssetMetadata,
    pub bytes: Vec<u8>,
}

impl OverlayAssetMedia {
    pub fn png(
        alt_text: impl Into<String>,
        pixel_width: u32,
        pixel_height: u32,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            metadata: OverlayAssetMetadata {
                media_type: OverlayMediaType::Png,
                pixel_width,
                pixel_height,
                alt_text: alt_text.into(),
            },
            bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAssetSource {
    Static(OverlayAssetMedia),
    Failure { message: String },
}

#[derive(Debug)]
pub struct OverlayAssetSnapshot<'a> {
    pub asset_ref: &'a OverlayAssetRef,
    pub metadata: &'a OverlayAssetMetadata,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAssetError {
    UnknownContentKey { key: String },
    MaterializationFailed { key: String, message: String },
    ReleasedAsset { asset_ref: String, key: String },
}

impl fmt::Display for OverlayAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownContentKey { key } => {
                write!(f, "overlay asset content key is unknown: {key}")
            }
            Self::MaterializationFailed { key, message } => {
                write!(
                    f,
                    "overlay asset materialization failed for {key}: {message}"
                )
            }
            Self::ReleasedAsset { asset_ref, key } => {
                write!(
                    f,
                    "overlay asset was released before resolve: asset_ref={asset_ref}, key={key}"
                )
            }
        }
    }
}

pub trait OverlayAssetStoreService {
    fn materialize(
        &mut self,
        key: &OverlayContentKey,
    ) -> Result<OverlayAssetRef, OverlayAssetError>;
    fn resolve<'a>(
        &'a self,
        asset_ref: &'a OverlayAssetRef,
    ) -> Result<OverlayAssetSnapshot<'a>, OverlayAssetError>;
    fn release_unused(&mut self, active_assets: &[OverlayAssetRef]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterializedOverlayAsset {
    key: OverlayContentKey,
    asset_ref: OverlayAssetRef,
    media: OverlayAssetMedia,
}

#[derive(Debug, Default)]
pub struct OverlayAssetStore {
    registered: BTreeMap<OverlayContentKey, OverlayAssetSource>,
    materialized_by_id: BTreeMap<String, MaterializedOverlayAsset>,
    materialized_by_key: BTreeMap<OverlayContentKey, String>,
    released_keys_by_id: BTreeMap<String, String>,
    next_id: u64,
}

impl OverlayAssetStore {
    pub fn register_asset(&mut self, key: OverlayContentKey, source: OverlayAssetSource) {
        log::debug!(
            "[overlay_asset_store] registering overlay asset source: key={}",
            key.describe()
        );
        self.registered.insert(key, source);
    }

    fn build_asset_ref(&mut self, key: &OverlayContentKey) -> OverlayAssetRef {
        self.next_id += 1;
        OverlayAssetRef {
            id: format!("overlay-{}-{}", self.next_id, sanitize_key(key)),
        }
    }
}

impl OverlayAssetStoreService for OverlayAssetStore {
    fn materialize(
        &mut self,
        key: &OverlayContentKey,
    ) -> Result<OverlayAssetRef, OverlayAssetError> {
        if let Some(existing_id) = self.materialized_by_key.get(key) {
            if let Some(existing) = self.materialized_by_id.get(existing_id) {
                log::debug!(
                    "[overlay_asset_store] reusing already materialized overlay asset: key={}, asset_ref={}",
                    key.describe(),
                    existing.asset_ref.id
                );
                return Ok(existing.asset_ref.clone());
            }
        }

        let source = self.registered.get(key).cloned().ok_or_else(|| {
            OverlayAssetError::UnknownContentKey {
                key: key.describe(),
            }
        })?;
        let media = match source {
            OverlayAssetSource::Static(media) => media,
            OverlayAssetSource::Failure { message } => {
                return Err(OverlayAssetError::MaterializationFailed {
                    key: key.describe(),
                    message,
                });
            }
        };
        let asset_ref = self.build_asset_ref(key);
        let materialized = MaterializedOverlayAsset {
            key: key.clone(),
            asset_ref: asset_ref.clone(),
            media,
        };
        log::debug!(
            "[overlay_asset_store] materialized overlay asset: key={}, asset_ref={}",
            key.describe(),
            asset_ref.id
        );
        self.materialized_by_key
            .insert(key.clone(), asset_ref.id.clone());
        self.materialized_by_id
            .insert(asset_ref.id.clone(), materialized);
        Ok(asset_ref)
    }

    fn resolve<'a>(
        &'a self,
        asset_ref: &'a OverlayAssetRef,
    ) -> Result<OverlayAssetSnapshot<'a>, OverlayAssetError> {
        let Some(materialized) = self.materialized_by_id.get(&asset_ref.id) else {
            return Err(OverlayAssetError::ReleasedAsset {
                asset_ref: asset_ref.id.clone(),
                key: self
                    .released_keys_by_id
                    .get(&asset_ref.id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
            });
        };
        Ok(OverlayAssetSnapshot {
            asset_ref: &materialized.asset_ref,
            metadata: &materialized.media.metadata,
            bytes: &materialized.media.bytes,
        })
    }

    fn release_unused(&mut self, active_assets: &[OverlayAssetRef]) {
        let active_ids = active_assets
            .iter()
            .map(|asset_ref| asset_ref.id.clone())
            .collect::<BTreeSet<_>>();
        let stale_ids = self
            .materialized_by_id
            .keys()
            .filter(|id| !active_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for stale_id in stale_ids {
            if let Some(stale) = self.materialized_by_id.remove(&stale_id) {
                log::debug!(
                    "[overlay_asset_store] releasing stale overlay asset: key={}, asset_ref={}",
                    stale.key.describe(),
                    stale.asset_ref.id
                );
                self.released_keys_by_id
                    .insert(stale.asset_ref.id.clone(), stale.key.describe());
                self.materialized_by_key.remove(&stale.key);
            }
        }
    }
}

fn sanitize_key(key: &OverlayContentKey) -> String {
    key.describe()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_reports_materialization_failure_from_registered_source() {
        let mut store = OverlayAssetStore::default();
        let key = OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        };
        store.register_asset(
            key.clone(),
            OverlayAssetSource::Failure {
                message: "encoder crashed".to_string(),
            },
        );

        let error = store
            .materialize(&key)
            .expect_err("failing source should return a materialization error");

        assert!(error.to_string().contains("encoder crashed"));
    }
}
