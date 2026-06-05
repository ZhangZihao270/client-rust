// Copyright 2021 TiKV Project Authors. Licensed under Apache-2.0.

use derive_new::new;

use crate::proto::metapb;
use crate::Error;
use crate::Key;
use crate::Result;

/// The ID of a region
pub type RegionId = u64;
/// The ID of a store
pub type StoreId = u64;

/// The ID and version information of a region.
#[derive(Eq, PartialEq, Hash, Clone, Default, Debug)]
pub struct RegionVerId {
    /// The ID of the region
    pub id: RegionId,
    /// Conf change version, auto increment when add or remove peer
    pub conf_ver: u64,
    /// Region version, auto increment when split or merge
    pub ver: u64,
}

/// Information about a TiKV region and its leader.
///
/// In TiKV all data is partitioned by range. Each partition is called a region.
#[derive(new, Clone, Default, Debug, PartialEq)]
pub struct RegionWithLeader {
    pub region: metapb::Region,
    pub leader: Option<metapb::Peer>,
}

impl Eq for RegionWithLeader {}

impl RegionWithLeader {
    pub fn contains(&self, key: &Key) -> bool {
        let key: &[u8] = key.into();
        let start_key = &self.region.start_key;
        let end_key = &self.region.end_key;
        key >= start_key.as_slice() && (key < end_key.as_slice() || end_key.is_empty())
    }

    pub fn start_key(&self) -> Key {
        self.region.start_key.to_vec().into()
    }

    pub fn end_key(&self) -> Key {
        self.region.end_key.to_vec().into()
    }

    pub fn range(&self) -> (Key, Key) {
        (self.start_key(), self.end_key())
    }

    pub fn ver_id(&self) -> RegionVerId {
        let region = &self.region;
        let epoch = region.region_epoch.as_ref().unwrap();
        RegionVerId {
            id: region.id,
            conf_ver: epoch.conf_ver,
            ver: epoch.version,
        }
    }

    pub fn id(&self) -> RegionId {
        self.region.id
    }

    pub fn get_store_id(&self) -> Result<StoreId> {
        self.leader
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::LeaderNotFound {
                region: self.ver_id(),
            })
            .map(|s| s.store_id)
    }

    /// Pick the peer a weak read should be served from, preferring the replica
    /// closest to this client.
    ///
    /// Selection order:
    /// 1. The peer on the client's preferred (local) store, if that store hosts
    ///    a replica of this region. The preferred store is read from the
    ///    `WEAK_READ_PREFERRED_STORE` env var (a store id). This is a PoC stand-in
    ///    for proper locality — production would match PD store labels (zone/az).
    /// 2. Otherwise any non-leader peer (a follower, so the read stays local to
    ///    it and avoids the remote-leader RTT).
    /// 3. Otherwise the leader (single-replica / no-follower fallback).
    pub fn weak_read_peer(&self) -> Option<metapb::Peer> {
        let leader_store = self.leader.as_ref().map(|p| p.store_id);

        if let Some(preferred) = std::env::var("WEAK_READ_PREFERRED_STORE")
            .ok()
            .and_then(|s| s.parse::<StoreId>().ok())
        {
            if let Some(p) = self.region.peers.iter().find(|p| p.store_id == preferred) {
                return Some(p.clone());
            }
        }

        self.region
            .peers
            .iter()
            .find(|p| Some(p.store_id) != leader_store)
            .cloned()
            .or_else(|| self.leader.clone())
    }
}
