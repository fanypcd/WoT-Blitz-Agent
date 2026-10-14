//! DetachedTurret (Type5 class 8): authoritative owner and raw rigid-body presence.
//! No death/HP inference and no vehicle position filter. Unknown wire layouts or
//! conflicting entity identities are rejected for the whole entity.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::combat::St10Sample;

#[derive(Debug, Clone)]
pub struct DetachedTurretPresence {
    pub pose: St10Sample,
    pub t_out: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct DetachedTurretRecord {
    pub eid: u32,
    pub vehicle_eid: u32,
    pub presence: Vec<DetachedTurretPresence>,
}

/// Fed by ReplayModel's existing packet scan; Type10 poses reuse its shared index.
#[derive(Default)]
pub(crate) struct DetachedTurretScan {
    classes: HashMap<u32, u16>,
    detached_eids: HashSet<u32>,
    conflicts: HashSet<u32>,
    pending: HashSet<u32>,
    open: HashSet<u32>,
    records: BTreeMap<u32, DetachedTurretRecord>,
    stream_end: f32,
}

impl DetachedTurretScan {
    pub fn observe(&mut self, kind: u32, clock: f32, payload: &[u8]) {
        if clock.is_finite() && clock >= 0.0 {
            self.stream_end = self.stream_end.max(clock);
        }
        if payload.len() < 4 || !matches!(kind, 4 | 5 | 33) {
            return;
        }
        let eid = u32_at(payload, 0);
        match kind {
            33 => {
                if self.open.contains(&eid) {
                    self.conflicts.insert(eid);
                }
                self.pending.insert(eid);
            }
            4 => {
                self.pending.remove(&eid);
                if self.open.remove(&eid) {
                    if let Some(record) = self.records.get_mut(&eid) {
                        let span = record.presence.last_mut().unwrap();
                        if !clock.is_finite() || clock < span.pose.clock {
                            self.conflicts.insert(eid);
                        } else {
                            span.t_out = Some(clock);
                        }
                    }
                }
            }
            5 => {
                let paired = self.pending.remove(&eid);
                if payload.len() < 6 {
                    self.conflicts.insert(eid);
                    return;
                }
                let class = u16::from_le_bytes([payload[4], payload[5]]);
                if self
                    .classes
                    .insert(eid, class)
                    .is_some_and(|old| old != class)
                {
                    self.conflicts.insert(eid);
                }
                if class != 8 {
                    return;
                }
                self.detached_eids.insert(eid);
                let Some((owner, pose)) = materialization(clock, payload) else {
                    self.conflicts.insert(eid);
                    return;
                };
                if !paired || eid == 0 || owner == 0 || eid == owner || self.open.contains(&eid) {
                    self.conflicts.insert(eid);
                    return;
                }
                let record = self
                    .records
                    .entry(eid)
                    .or_insert_with(|| DetachedTurretRecord {
                        eid,
                        vehicle_eid: owner,
                        presence: Vec::new(),
                    });
                if record.vehicle_eid != owner
                    || record
                        .presence
                        .last()
                        .is_some_and(|last| last.t_out.is_none_or(|end| end >= clock))
                {
                    self.conflicts.insert(eid);
                    return;
                }
                record
                    .presence
                    .push(DetachedTurretPresence { pose, t_out: None });
                self.open.insert(eid);
            }
            _ => unreachable!(),
        }
    }

    pub fn finish(mut self) -> (Vec<DetachedTurretRecord>, HashSet<u32>) {
        // Two detached entities claiming one vehicle have no unambiguous model
        // ownership. Reject both, even if their recorded lifetimes don't overlap.
        let mut owner_counts: HashMap<u32, usize> = HashMap::new();
        for record in self.records.values() {
            *owner_counts.entry(record.vehicle_eid).or_default() += 1;
        }
        self.records.retain(|eid, record| {
            !self.conflicts.contains(eid)
                && !self.conflicts.contains(&record.vehicle_eid)
                && self.classes.get(&record.vehicle_eid) == Some(&2)
                && owner_counts[&record.vehicle_eid] == 1
        });
        for record in self.records.values_mut() {
            for span in &mut record.presence {
                if span.t_out.is_none() {
                    span.t_out = Some(self.stream_end);
                }
            }
        }
        (self.records.into_values().collect(), self.detached_eids)
    }
}

fn u32_at(p: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(p[o..o + 4].try_into().unwrap())
}
fn f32_at(p: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(p, o))
}

/// Observed in 83 class-8 materializations from 50 11.20 replays:
/// init length 6, one property, key 0, owning combat-vehicle entity id.
fn materialization(clock: f32, p: &[u8]) -> Option<(u32, St10Sample)> {
    if p.len() != 48 || u32_at(p, 38) != 6 || p[42..44] != [1, 0] {
        return None;
    }
    let pose = St10Sample {
        clock,
        pos: [f32_at(p, 14), f32_at(p, 18), f32_at(p, 22)],
        yaw: f32_at(p, 26),
        pitch: f32_at(p, 30),
        roll: f32_at(p, 34),
        pos_error: [0.0; 3], // not a Type10 error box; never consumed by vehicle filtering
    };
    valid_pose(&pose).then(|| (u32_at(p, 44), pose))
}

pub(crate) fn valid_pose(p: &St10Sample) -> bool {
    p.clock.is_finite()
        && p.clock >= 0.0
        && p.pos.iter().all(|v| v.is_finite())
        && [p.yaw, p.pitch, p.roll].iter().all(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(eid: u32, class: u16, owner: u32) -> Vec<u8> {
        let mut p = vec![0; 48];
        p[..4].copy_from_slice(&eid.to_le_bytes());
        p[4..6].copy_from_slice(&class.to_le_bytes());
        p[38..42].copy_from_slice(&6u32.to_le_bytes());
        p[42] = 1;
        p[44..48].copy_from_slice(&owner.to_le_bytes());
        p
    }
    fn scan(events: &[(u32, f32, Vec<u8>)]) -> Vec<DetachedTurretRecord> {
        let mut scan = DetachedTurretScan::default();
        scan.observe(5, 0.0, &create(1, 2, 0));
        for (kind, t, p) in events {
            scan.observe(*kind, *t, p);
        }
        scan.finish().0
    }
    fn entry(eid: u32, t: f32, owner: u32) -> Vec<(u32, f32, Vec<u8>)> {
        vec![
            (33, t - 0.1, eid.to_le_bytes().to_vec()),
            (5, t, create(eid, 8, owner)),
        ]
    }

    #[test]
    fn authoritative_owner_and_initial_full_pose_are_preserved() {
        let mut events = entry(20, 2.0, 1);
        for (i, v) in [1.0f32, 2.0, 3.0, -3.1, 0.7, 2.9].iter().enumerate() {
            events[1].2[14 + 4 * i..18 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        events.push((10, 9.0, vec![]));
        let r = scan(&events);
        assert_eq!((r[0].eid, r[0].vehicle_eid), (20, 1));
        let p = &r[0].presence[0].pose;
        assert_eq!(p.pos, [1.0, 2.0, 3.0]);
        assert_eq!((p.yaw, p.pitch, p.roll), (-3.1, 0.7, 2.9));
        assert_eq!(r[0].presence[0].t_out, Some(9.0));
        assert_eq!(scan(&entry(20, 2.0, 1))[0].presence[0].t_out, Some(2.0));
    }

    #[test]
    fn explicit_aoi_leave_and_reentry_create_disjoint_spans() {
        let mut e = entry(20, 2.0, 1);
        e.push((4, 3.0, 20u32.to_le_bytes().to_vec()));
        e.extend(entry(20, 3.1, 1));
        e.push((10, 9.0, vec![]));
        let r = scan(&e);
        assert_eq!(r[0].presence.len(), 2);
        assert_eq!(r[0].presence[0].t_out, Some(3.0));
        assert_eq!(r[0].presence[1].pose.clock, 3.1);
        assert_eq!(r[0].presence[1].t_out, Some(9.0));
    }

    #[test]
    fn ambiguous_owner_class_reuse_and_unpaired_creates_are_rejected() {
        assert!(scan(&entry(0, 2.0, 1)).is_empty());
        assert!(scan(&entry(20, 2.0, 9)).is_empty());
        assert!(scan(&entry(20, 2.0, 0)).is_empty());
        assert!(scan(&entry(20, 2.0, 20)).is_empty());
        assert!(scan(&[(5, 2.0, create(20, 8, 1))]).is_empty());
        let mut e = entry(20, 2.0, 1);
        e.push((4, 3.0, 20u32.to_le_bytes().to_vec()));
        e.extend(entry(20, 4.0, 9));
        assert!(scan(&e).is_empty());
        let mut e = entry(20, 2.0, 1);
        e.push((5, 3.0, create(20, 3, 0)));
        assert!(scan(&e).is_empty());
        let mut e = entry(20, 2.0, 1);
        e.extend(entry(21, 3.0, 1));
        assert!(scan(&e).is_empty());
    }

    #[test]
    fn unknown_initialization_layout_and_nonfinite_evidence_are_rejected() {
        for offset in [38, 42, 43] {
            let mut e = entry(20, 2.0, 1);
            e[1].2[offset] ^= 1;
            assert!(scan(&e).is_empty());
        }
        for offset in [14, 26, 30, 34] {
            let mut e = entry(20, 2.0, 1);
            e[1].2[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
            assert!(scan(&e).is_empty());
        }
        let mut e = entry(20, 2.0, 1);
        e[1].2.push(0);
        assert!(scan(&e).is_empty());
        e[1].2.truncate(47);
        assert!(scan(&e).is_empty());
        let mut e = entry(20, 2.0, 1);
        e.push((4, 1.0, 20u32.to_le_bytes().to_vec()));
        assert!(scan(&e).is_empty());
    }
}
