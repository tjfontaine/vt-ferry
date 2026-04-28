//! Indexes launcher-provided IOSurfaces by pool shape so `vt-real` can match
//! an incoming `CREATE_BUFFER_POOL` request against a pre-allocated
//! zero-copy IOSurface.
//!
//! The launcher emits `VT_FERRY_IOSURFACE_POOL_SPECS_JSON` naming each pool
//! that has `zero_copy: true` (width, height, pixel_format, slot_count,
//! iosurface_id) and registers the corresponding Mach ports via
//! `mach_ports_register` before `posix_spawn`'ing the worker. The worker
//! pulls those ports out via `mach_ports_lookup` (see `IOSurfaceRegistry`)
//! and hands them to this directory, which pairs each surface with its
//! declared shape.

use crate::iosurface_bridge::{IOSurfaceRegistry, OwnedIOSurface};
use serde::Deserialize;
use std::env;

pub const ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON: &str = "VT_FERRY_IOSURFACE_POOL_SPECS_JSON";

#[derive(Clone, Debug, Deserialize)]
pub struct IOSurfacePoolSpec {
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    #[serde(default = "one")]
    pub slot_count: u32,
    pub iosurface_id: u32,
}

fn one() -> u32 {
    1
}

/// A pool pairs a single launcher-allocated IOSurface with its declared
/// shape. The IOSurface is sized `slot_count × per_slot_bytes` — a raw
/// byte region carrying every slot's pixels packed contiguously. The
/// worker addresses each slot by byte offset within the surface.
pub struct IOSurfacePool {
    pub spec: IOSurfacePoolSpec,
    pub surface: OwnedIOSurface,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct PoolShape {
    width: u32,
    height: u32,
    pixel_format: u32,
}

/// One launcher-registered IOSurface entry plus a "claimed by some
/// active connection?" flag. When a connection's
/// `OP_CREATE_BUFFER_POOL` claims an entry, `claimed` flips true;
/// claims persist until the claiming backend drops, at which
/// point its `Drop` impl calls `release_entry` to flip it back.
struct DirectoryEntry {
    shape: PoolShape,
    pool: IOSurfacePool,
    claimed: bool,
}

pub struct IOSurfacePoolDirectory {
    // Vec rather than HashMap because we allow multiple distinct
    // entries at the same shape. Two concurrent guests competing
    // for surfaces of the same shape need two distinct entries —
    // sharing a single underlying IOSurface across concurrent
    // writers would let them stomp each other's frames.
    //
    // Lookup is linear; that's fine because the directory has
    // at most TASK_PORT_REGISTER_MAX = 3 entries (Mach port
    // registration limit), and `take_matching` is on the
    // CREATE_BUFFER_POOL path, not a per-frame hot path.
    entries: Vec<DirectoryEntry>,
}

impl IOSurfacePoolDirectory {
    pub fn empty() -> Self {
        IOSurfacePoolDirectory {
            entries: Vec::new(),
        }
    }

    /// Parse `VT_FERRY_IOSURFACE_POOL_SPECS_JSON` from the environment and
    /// consume the matching IOSurfaces from the given registry. One JSON
    /// entry per pool — the IOSurface itself is the packed byte region
    /// that holds every slot.
    ///
    /// Multiple specs at the same shape are kept (no dedup). Two
    /// concurrent connections asking for that shape each claim a
    /// different entry; if N entries exist and an N+1th connection
    /// asks, `take_matching` returns None and the worker rejects with
    /// `STATUS_UNSUPPORTED_CODEC_OR_FORMAT`.
    pub fn load(registry: &mut IOSurfaceRegistry) -> Self {
        let raw = match env::var(ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON) {
            Ok(v) if !v.is_empty() => v,
            _ => return Self::empty(),
        };

        let specs: Vec<IOSurfacePoolSpec> = match serde_json::from_str(&raw) {
            Ok(specs) => specs,
            Err(error) => {
                eprintln!(
                    "IOSurfacePoolDirectory: failed to parse {}: {}",
                    ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON, error
                );
                return Self::empty();
            }
        };

        let mut entries: Vec<DirectoryEntry> = Vec::new();
        for spec in specs {
            let surface = match registry.take_by_id(spec.iosurface_id) {
                Some(s) => s,
                None => {
                    eprintln!(
                        "IOSurfacePoolDirectory: no registered IOSurface with id \
                         {} for shape {}x{} pf=0x{:x} slots={}",
                        spec.iosurface_id,
                        spec.width,
                        spec.height,
                        spec.pixel_format,
                        spec.slot_count,
                    );
                    continue;
                }
            };
            let shape = PoolShape {
                width: spec.width,
                height: spec.height,
                pixel_format: spec.pixel_format,
            };
            entries.push(DirectoryEntry {
                shape,
                pool: IOSurfacePool { spec, surface },
                claimed: false,
            });
        }
        IOSurfacePoolDirectory { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Claim the first unclaimed pool matching the given shape.
    /// Returns `(entry_id, IOSurfacePool with retained surface)`.
    /// The caller keeps the `entry_id` and passes it back to
    /// `release_entry` when its connection ends, freeing the slot
    /// for the next connection. Concurrent connections at the
    /// same shape compete for unclaimed entries; the N+1th
    /// concurrent claim at a shape with N entries returns None.
    ///
    /// Within a single connection, FFmpeg's repeated
    /// CREATE_BUFFER_POOL calls (sometimes the encoder issues
    /// two against the same session) DO each consume an entry —
    /// the worker's pools map already keeps them distinct. The
    /// previous "share one surface across pool calls" semantic
    /// has been retired because it allowed concurrent guests to
    /// silently corrupt each other's encode-input data.
    ///
    /// `requested_slot_count` is ignored — caller caps max_buffers
    /// at the pool's actual slot_count.
    pub fn take_matching(
        &mut self,
        width: u32,
        height: u32,
        pixel_format: u32,
        _requested_slot_count: u32,
    ) -> Option<(usize, IOSurfacePool)> {
        for (id, entry) in self.entries.iter_mut().enumerate() {
            if !entry.claimed
                && entry.shape.width == width
                && entry.shape.height == height
                && entry.shape.pixel_format == pixel_format
            {
                entry.claimed = true;
                return Some((
                    id,
                    IOSurfacePool {
                        spec: entry.pool.spec.clone(),
                        surface: entry.pool.surface.clone_ref(),
                    },
                ));
            }
        }
        None
    }

    /// Release a previously-claimed entry by id so a future
    /// `take_matching` at the same shape can re-claim it. Called
    /// from `VtRealBackend::Drop` for each entry the backend
    /// claimed during its lifetime.
    ///
    /// Safe to call on an already-released id (idempotent) — the
    /// claimed flag is just flipped back to false.
    pub fn release_entry(&mut self, id: usize) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.claimed = false;
        }
    }

    /// Test-only: seed the directory with a manually constructed pool.
    #[cfg(test)]
    pub fn install_for_tests(&mut self, pool: IOSurfacePool) {
        let shape = PoolShape {
            width: pool.spec.width,
            height: pool.spec.height,
            pixel_format: pool.spec.pixel_format,
        };
        self.entries.push(DirectoryEntry {
            shape,
            pool,
            claimed: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_env() {
        let raw = "";
        let parsed: Result<Vec<IOSurfacePoolSpec>, _> = serde_json::from_str(raw);
        assert!(parsed.is_err(), "empty string should not parse");
    }

    #[test]
    fn parses_single_entry_with_slot_count_default() {
        let raw = r#"[{"width":256,"height":144,"pixel_format":1111970369,"iosurface_id":42}]"#;
        let parsed: Vec<IOSurfacePoolSpec> = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].width, 256);
        assert_eq!(parsed[0].height, 144);
        assert_eq!(parsed[0].slot_count, 1);
        assert_eq!(parsed[0].iosurface_id, 42);
    }

    #[test]
    fn parses_explicit_slot_count() {
        let raw = r#"[{"width":256,"height":144,"pixel_format":0x42475241,
                      "slot_count":4,"iosurface_id":7}]"#
            .replace("0x42475241", "1111970369");
        let parsed: Vec<IOSurfacePoolSpec> = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed[0].slot_count, 4);
    }

    #[test]
    fn take_matching_returns_none_for_empty_directory() {
        let mut dir = IOSurfacePoolDirectory::empty();
        assert!(dir.take_matching(256, 144, 0x42475241, 1).is_none());
    }
}
