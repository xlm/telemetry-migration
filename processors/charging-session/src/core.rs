use std::collections::{BTreeMap, HashSet};

use crate::types::{
    ChargingEvent, ChargingState, ChargingSummary, CloseReason, DlqReason, DlqRecord, Emitted,
};

const IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Default)]
pub struct Processor {
    vehicles: BTreeMap<String, Vehicle>,
}

#[derive(Debug)]
struct Vehicle {
    clock: i64,
    sessions: BTreeMap<String, Session>,
    closed: HashSet<String>,
    seen: HashSet<i64>,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            clock: i64::MIN,
            sessions: BTreeMap::new(),
            closed: HashSet::new(),
            seen: HashSet::new(),
        }
    }
}

#[derive(Debug)]
struct Session {
    events: Vec<ValidEvent>,
    last_event_ts: i64,
}

#[derive(Debug)]
struct ValidEvent {
    event_id: String,
    vehicle_id: String,
    session_id: Option<String>,
    event_ts: i64,
    ingest_ts: i64,
    seq: i64,
    state: ChargingState,
    soc: Option<f64>,
    power: Option<f64>,
}

fn validate(event: ChargingEvent) -> Result<ValidEvent, DlqRecord> {
    let rejected = |reason| DlqRecord {
        event_id: event.event_id.clone(),
        vehicle_id: event.vehicle_id.clone(),
        reason,
    };
    let (Some(event_id), Some(vehicle_id), Some(event_ts), Some(ingest_ts), Some(seq), Some(state)) = (
        &event.event_id,
        &event.vehicle_id,
        event.event_ts,
        event.ingest_ts,
        event.seq,
        &event.charging_state,
    ) else {
        return Err(rejected(DlqReason::MissingField));
    };
    let state = ChargingState::from_wire(state).ok_or_else(|| rejected(DlqReason::UnknownState))?;
    if event.session_id.is_none() && state != ChargingState::Heartbeat {
        return Err(rejected(DlqReason::MissingField));
    }
    if event.soc_pct.is_some_and(|soc| soc < 0.0 || soc > 100.0) {
        return Err(rejected(DlqReason::SocOutOfRange));
    }
    if event.power_kw.is_some_and(|power| power < 0.0) {
        return Err(rejected(DlqReason::NegativePower));
    }
    Ok(ValidEvent {
        event_id: event_id.clone(),
        vehicle_id: vehicle_id.clone(),
        session_id: event.session_id,
        event_ts,
        ingest_ts,
        seq,
        state,
        soc: event.soc_pct,
        power: event.power_kw,
    })
}

impl Processor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_batch(
        &mut self,
        events: impl IntoIterator<Item = ChargingEvent>,
    ) -> Vec<Emitted> {
        let mut output = Vec::new();
        let mut valid = Vec::new();
        for event in events {
            match validate(event) {
                Ok(event) => valid.push(event),
                Err(record) => output.push(Emitted::Dlq(record)),
            }
        }
        valid.sort_by(|a, b| {
            (&a.vehicle_id, a.ingest_ts)
                .cmp(&(&b.vehicle_id, b.ingest_ts))
                .then_with(|| a.event_id.encode_utf16().cmp(b.event_id.encode_utf16()))
        });
        for event in valid {
            output.extend(self.on_valid_event(event));
        }
        output
    }

    fn on_valid_event(&mut self, event: ValidEvent) -> Vec<Emitted> {
        let vehicle = self.vehicles.entry(event.vehicle_id.clone()).or_default();
        vehicle.clock = vehicle.clock.max(event.event_ts);
        let mut expired: Vec<_> = vehicle
            .sessions
            .iter()
            .filter(|(_, session)| {
                session.last_event_ts.wrapping_add(IDLE_TIMEOUT_MS) <= vehicle.clock
            })
            .map(|(id, _)| id.clone())
            .collect();
        expired.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let mut output = Vec::new();
        for id in expired {
            let session = vehicle.sessions.remove(&id).expect("open session");
            output.push(Emitted::Summary(summarise(
                &id,
                &event.vehicle_id,
                session,
                CloseReason::Timeout,
            )));
            vehicle.closed.insert(id);
        }
        if event.state == ChargingState::Heartbeat {
            return output;
        }
        let id = event.session_id.as_deref().expect("validated session");
        let reason = if vehicle.seen.contains(&event.seq) {
            Some(DlqReason::Duplicate)
        } else if vehicle.closed.contains(id) {
            Some(DlqReason::Late)
        } else {
            None
        };
        if let Some(reason) = reason {
            output.push(Emitted::Dlq(DlqRecord {
                event_id: Some(event.event_id),
                vehicle_id: Some(event.vehicle_id),
                reason,
            }));
            return output;
        }
        let id = id.to_owned();
        let session = vehicle
            .sessions
            .entry(id.clone())
            .or_insert_with(|| Session {
                events: Vec::new(),
                last_event_ts: event.event_ts,
            });
        session.last_event_ts = session.last_event_ts.max(event.event_ts);
        vehicle.seen.insert(event.seq);
        let close_reason = match event.state {
            ChargingState::Complete => Some(CloseReason::Complete),
            ChargingState::Unplugged => Some(CloseReason::Unplugged),
            _ => None,
        };
        let vehicle_id = event.vehicle_id.clone();
        session.events.push(event);
        if let Some(reason) = close_reason {
            let session = vehicle.sessions.remove(&id).expect("open session");
            output.push(Emitted::Summary(summarise(
                &id,
                &vehicle_id,
                session,
                reason,
            )));
            vehicle.closed.insert(id);
        }
        output
    }
}

fn summarise(
    id: &str,
    vehicle_id: &str,
    mut session: Session,
    reason: CloseReason,
) -> ChargingSummary {
    session.events.sort_by(|a, b| {
        a.event_ts
            .cmp(&b.event_ts)
            .then_with(|| a.event_id.encode_utf16().cmp(b.event_id.encode_utf16()))
    });
    let events = &session.events;
    let start_ts = events.first().expect("nonempty session").event_ts;
    let end_ts = events.last().expect("nonempty session").event_ts;
    let energy = events.windows(2).fold(0.0, |total, pair| {
        let [first, second] = pair else {
            unreachable!()
        };
        if first.state == ChargingState::Charging {
            total
                + 0.5
                    * (first.power.unwrap_or(0.0) + second.power.unwrap_or(0.0))
                    * second.event_ts.wrapping_sub(first.event_ts) as f64
                    / 3_600_000.0
        } else {
            total
        }
    });
    ChargingSummary {
        session_id: id.to_owned(),
        vehicle_id: vehicle_id.to_owned(),
        start_ts,
        end_ts,
        duration_s: (end_ts.wrapping_sub(start_ts) / 1000) as i32 as i64,
        start_soc_pct: events.iter().find_map(|event| event.soc),
        end_soc_pct: events.iter().rev().find_map(|event| event.soc),
        energy_kwh: round_energy(energy),
        peak_power_kw: events
            .iter()
            .filter_map(|event| event.power)
            .fold(0.0, f64::max),
        event_count: events.len() as u64,
        close_reason: reason,
        schema_version: "1".to_owned(),
    }
}

fn round_energy(energy: f64) -> f64 {
    // Scala rounds BigDecimal(double), not the double's binary expansion.
    let decimal = energy.abs().to_string();
    let (whole, fraction) = decimal.split_once('.').unwrap_or((&decimal, ""));
    if fraction.len() <= 3 {
        return energy;
    }
    let mut millis: u64 = format!("{whole}{}", &fraction[..3])
        .parse()
        .expect("finite nonnegative energy with fractional precision");
    let tail = &fraction[3..];
    if tail.as_bytes()[0] > b'5'
        || (tail.as_bytes()[0] == b'5'
            && (tail[1..].bytes().any(|digit| digit != b'0') || millis % 2 == 1))
    {
        millis += 1;
    }
    let rounded: f64 = format!("{millis}e-3").parse().expect("rounded energy");
    rounded.copysign(energy)
}

#[cfg(test)]
mod tests {
    use super::{round_energy, validate, Processor, IDLE_TIMEOUT_MS};
    use crate::types::{ChargingEvent, ChargingSummary, CloseReason, DlqReason, Emitted, Location};

    fn event(
        id: &str,
        session: &str,
        state: &str,
        ts: i64,
        ingest: i64,
        seq: i64,
    ) -> ChargingEvent {
        ChargingEvent {
            event_id: Some(id.to_owned()),
            vehicle_id: Some("VH-1".to_owned()),
            session_id: Some(session.to_owned()),
            event_ts: Some(ts),
            ingest_ts: Some(ingest),
            seq: Some(seq),
            charging_state: Some(state.to_owned()),
            soc_pct: Some(20.0),
            power_kw: Some(60.0),
            schema_version: Some("99".to_owned()),
            ..ChargingEvent::default()
        }
    }

    fn summaries(output: &[Emitted]) -> Vec<ChargingSummary> {
        output
            .iter()
            .filter_map(|record| match record {
                Emitted::Summary(summary) => Some(summary.clone()),
                _ => None,
            })
            .collect()
    }

    fn reasons(output: &[Emitted]) -> Vec<DlqReason> {
        output
            .iter()
            .filter_map(|record| match record {
                Emitted::Dlq(record) => Some(record.reason),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn complete_summary_derives_every_field_and_discards_location_and_input_version() {
        let mut start = event("a", "CS-1", "CHARGING", 1000, 1, 1);
        start.location = Some(Location { lat: 1.0, lon: 2.0 });
        let mut end = event("b", "CS-1", "COMPLETE", 61_999, 2, 2);
        end.soc_pct = Some(21.0);
        let output = Processor::new().process_batch([start, end]);
        assert!(reasons(&output).is_empty());
        assert_eq!(
            summaries(&output),
            [ChargingSummary {
                session_id: "CS-1".to_owned(),
                vehicle_id: "VH-1".to_owned(),
                start_ts: 1000,
                end_ts: 61_999,
                duration_s: 60,
                start_soc_pct: Some(20.0),
                end_soc_pct: Some(21.0),
                energy_kwh: 1.017,
                peak_power_kw: 60.0,
                event_count: 2,
                close_reason: CloseReason::Complete,
                schema_version: "1".to_owned(),
            }]
        );
    }

    #[test]
    fn every_required_field_is_presence_checked_and_identifiers_are_preserved() {
        for field in [
            "event_id",
            "vehicle_id",
            "session_id",
            "event_ts",
            "ingest_ts",
            "seq",
            "charging_state",
        ] {
            let valid = event("a", "s", "CHARGING", 0, 0, 0);
            let mut value = serde_json::to_value(&valid).unwrap();
            value.as_object_mut().unwrap().remove(field);
            let invalid: ChargingEvent = serde_json::from_value(value).unwrap();
            let rejected = validate(invalid).unwrap_err();
            assert_eq!(rejected.reason, DlqReason::MissingField, "{field}");
            assert_eq!(
                rejected.event_id,
                if field == "event_id" {
                    None
                } else {
                    valid.event_id
                }
            );
            assert_eq!(
                rejected.vehicle_id,
                if field == "vehicle_id" {
                    None
                } else {
                    valid.vehicle_id
                }
            );
        }
    }

    #[test]
    fn validation_preserves_first_failing_reason() {
        let mut e = event("a", "s", "invalid", 0, 0, 0);
        e.session_id = None;
        e.soc_pct = Some(-1.0);
        e.power_kw = Some(-1.0);
        e.event_ts = None;
        assert_eq!(
            validate(e.clone()).unwrap_err().reason,
            DlqReason::MissingField
        );
        e.event_ts = Some(0);
        assert_eq!(
            validate(e.clone()).unwrap_err().reason,
            DlqReason::UnknownState
        );
        e.charging_state = Some("CHARGING".to_owned());
        assert_eq!(
            validate(e.clone()).unwrap_err().reason,
            DlqReason::MissingField
        );
        e.session_id = Some("s".to_owned());
        assert_eq!(
            validate(e.clone()).unwrap_err().reason,
            DlqReason::SocOutOfRange
        );
        e.soc_pct = Some(50.0);
        assert_eq!(validate(e).unwrap_err().reason, DlqReason::NegativePower);
    }

    #[test]
    fn numeric_bounds_and_exact_state_names_are_not_coerced() {
        for soc in [-0.001, 100.001] {
            let mut e = event("a", "s", "CHARGING", 0, 0, 0);
            e.soc_pct = Some(soc);
            assert_eq!(validate(e).unwrap_err().reason, DlqReason::SocOutOfRange);
        }
        for soc in [None, Some(0.0), Some(100.0)] {
            for power in [None, Some(-0.0), Some(0.0), Some(1000.0)] {
                let mut e = event("", "", "CHARGING", -1, -1, -1);
                e.vehicle_id = Some("".to_owned());
                e.soc_pct = soc;
                e.power_kw = power;
                assert!(validate(e).is_ok());
            }
        }
        for state in [
            "charging",
            " CHARGING",
            "CHARGING ",
            "CHARGING!",
            "CHARGI\u{301}NG",
            "",
            "UNKNOWN",
        ] {
            assert_eq!(
                validate(event("a", "s", state, 0, 0, 0))
                    .unwrap_err()
                    .reason,
                DlqReason::UnknownState
            );
        }
    }

    #[test]
    fn invalid_events_neither_advance_clock_nor_reserve_sequence() {
        let mut invalid = event("b", "s", "CHARGING", IDLE_TIMEOUT_MS, 2, 2);
        invalid.soc_pct = Some(101.0);
        let output = Processor::new().process_batch([
            event("a", "s", "CHARGING", 0, 1, 1),
            invalid,
            event("c", "s", "COMPLETE", 60_000, 3, 2),
        ]);
        assert_eq!(reasons(&output), [DlqReason::SocOutOfRange]);
        assert_eq!(summaries(&output)[0].close_reason, CloseReason::Complete);
        assert_eq!(summaries(&output)[0].event_count, 2);
    }

    #[test]
    fn ingest_order_then_event_id_determines_duplicate_winner_not_file_or_event_time() {
        let output = Processor::new().process_batch([
            event("z", "s", "CHARGING", 0, 2, 1),
            event("b", "s", "CHARGING", 1000, 1, 1),
            event("a", "s", "PLUGGED_IN", 2000, 1, 1),
            event("c", "s", "COMPLETE", 3000, 3, 2),
        ]);
        assert_eq!(
            reasons(&output),
            [DlqReason::Duplicate, DlqReason::Duplicate]
        );
        let summary = &summaries(&output)[0];
        assert_eq!(summary.start_ts, 2000);
        assert_eq!(summary.energy_kwh, 0.0);
    }

    #[test]
    fn dedup_is_vehicle_wide_not_session_or_event_id_and_persists_after_close() {
        let mut other_vehicle = event("same", "s", "COMPLETE", 2, 4, 1);
        other_vehicle.vehicle_id = Some("VH-2".to_owned());
        let output = Processor::new().process_batch([
            event("same", "s", "COMPLETE", 0, 1, 1),
            event("new-id", "other", "COMPLETE", 1, 2, 1),
            event("same", "other", "COMPLETE", 2, 3, 2),
            other_vehicle,
        ]);
        assert_eq!(reasons(&output), [DlqReason::Duplicate]);
        assert_eq!(summaries(&output).len(), 3);
    }

    #[test]
    fn duplicate_precedes_late_and_late_does_not_reserve_sequence() {
        let output = Processor::new().process_batch([
            event("a", "s", "COMPLETE", 10, 1, 1),
            event("b", "s", "CHARGING", 0, 2, 1),
            event("c", "s", "CHARGING", 20, 3, 2),
            event("d", "other", "UNPLUGGED", 30, 4, 2),
        ]);
        assert_eq!(reasons(&output), [DlqReason::Duplicate, DlqReason::Late]);
        assert_eq!(summaries(&output)[1].close_reason, CloseReason::Unplugged);
    }

    #[test]
    fn out_of_order_events_are_sorted_for_energy_and_soc_including_id_ties() {
        let mut b = event("b", "s", "CHARGING", 60_000, 1, 1);
        b.soc_pct = Some(30.0);
        let mut a = event("a", "s", "CHARGING", 0, 2, 2);
        a.soc_pct = Some(10.0);
        let mut c = event("c", "s", "PAUSED", 60_000, 3, 3);
        c.soc_pct = Some(40.0);
        let mut end = event("d", "s", "COMPLETE", 120_000, 4, 4);
        end.soc_pct = None;
        let output = Processor::new().process_batch([b, a, c, end]);
        let summary = &summaries(&output)[0];
        assert_eq!((summary.start_ts, summary.end_ts), (0, 120_000));
        assert_eq!(
            (summary.start_soc_pct, summary.end_soc_pct),
            (Some(10.0), Some(40.0))
        );
        assert_eq!(summary.energy_kwh, 1.0);
        assert_eq!(summary.event_count, 4);
    }

    #[test]
    fn soc_uses_present_values_and_empty_soc_serializes_as_null() {
        let mut start = event("a", "s", "PLUGGED_IN", 0, 1, 1);
        start.soc_pct = None;
        let mut end = event("d", "s", "UNPLUGGED", 3000, 4, 4);
        end.soc_pct = None;
        let output = Processor::new().process_batch([
            start.clone(),
            event("b", "s", "CHARGING", 1000, 2, 2),
            event("c", "s", "PAUSED", 2000, 3, 3),
            end.clone(),
        ]);
        assert_eq!(summaries(&output)[0].start_soc_pct, Some(20.0));
        assert_eq!(summaries(&output)[0].end_soc_pct, Some(20.0));
        let output = Processor::new().process_batch([start, end]);
        let value = serde_json::to_value(&summaries(&output)[0]).unwrap();
        assert!(value["start_soc_pct"].is_null());
        assert!(value["end_soc_pct"].is_null());
    }

    #[test]
    fn missing_power_is_zero_trapezoids_use_both_endpoints_and_skip_paused_intervals() {
        let mut start = event("a", "s", "CHARGING", 0, 1, 1);
        start.power_kw = None;
        let mut paused = event("b", "s", "PAUSED", 60_000, 2, 2);
        paused.power_kw = Some(120.0);
        let mut resume = event("c", "s", "CHARGING", 120_000, 3, 3);
        resume.power_kw = Some(60.0);
        let mut end = event("d", "s", "COMPLETE", 180_000, 4, 4);
        end.power_kw = None;
        let output = Processor::new().process_batch([start, paused, resume, end]);
        assert_eq!(summaries(&output)[0].energy_kwh, 1.5);
        assert_eq!(summaries(&output)[0].peak_power_kw, 120.0);
    }

    #[test]
    fn energy_rounds_decimal_half_even_once_after_summing() {
        for (input, expected) in [
            (0.0005, 0.0),
            (0.0015, 0.002),
            (1.2345, 1.234),
            (1.2355, 1.236),
            (1.2345_f64.next_down(), 1.234),
            (1.2345_f64.next_up(), 1.235),
            (1.9995, 2.0),
            (1e-20, 0.0),
            (1e20, 1e20),
            (-1.2345, -1.234),
        ] {
            assert_eq!(round_energy(input), expected, "{input}");
        }
        let mut a = event("a", "s", "CHARGING", 0, 1, 1);
        let mut b = event("b", "s", "CHARGING", 60_000, 2, 2);
        let mut c = event("c", "s", "COMPLETE", 120_000, 3, 3);
        for e in [&mut a, &mut b, &mut c] {
            e.power_kw = Some(74.07);
        }
        let output = Processor::new().process_batch([a, b.clone()]);
        assert!(output.is_empty());
        b.charging_state = Some("COMPLETE".to_owned());
        let mut a = event("a", "s", "CHARGING", 0, 1, 1);
        a.power_kw = Some(74.07);
        assert_eq!(
            summaries(&Processor::new().process_batch([a.clone(), b.clone()]))[0].energy_kwh,
            1.234
        );
        b.charging_state = Some("CHARGING".to_owned());
        assert_eq!(
            summaries(&Processor::new().process_batch([a, b, c]))[0].energy_kwh,
            2.469
        );
    }

    #[test]
    fn any_nonheartbeat_state_can_start_including_single_event_terminal_sessions() {
        for state in ["PLUGGED_IN", "CHARGING", "PAUSED", "COMPLETE", "UNPLUGGED"] {
            let mut processor = Processor::new();
            let output = processor.process_batch([event("a", "s", state, 0, 1, 1)]);
            if state == "COMPLETE" || state == "UNPLUGGED" {
                assert_eq!(summaries(&output)[0].event_count, 1);
                assert_eq!(summaries(&output)[0].duration_s, 0);
                assert_eq!(summaries(&output)[0].energy_kwh, 0.0);
            } else {
                assert!(output.is_empty());
                assert!(processor.process_batch([]).is_empty());
            }
        }
    }

    #[test]
    fn heartbeat_expires_at_boundary_without_becoming_an_event_or_dedup_key() {
        let mut processor = Processor::new();
        processor.process_batch([event("a", "s", "CHARGING", 1000, 1, 1)]);
        let mut heartbeat = event("b", "s", "HEARTBEAT", IDLE_TIMEOUT_MS + 999, 2, 1);
        heartbeat.session_id = None;
        assert!(processor.process_batch([heartbeat.clone()]).is_empty());
        heartbeat.event_ts = Some(IDLE_TIMEOUT_MS + 1000);
        let output = processor.process_batch([heartbeat]);
        assert_eq!(output.len(), 1);
        let summary = &summaries(&output)[0];
        assert_eq!(summary.close_reason, CloseReason::Timeout);
        assert_eq!((summary.start_ts, summary.end_ts), (1000, 1000));
        assert_eq!(summary.event_count, 1);
        let output = processor.process_batch([
            event("c", "s", "HEARTBEAT", IDLE_TIMEOUT_MS + 1000, 3, 2),
            event("d", "new", "COMPLETE", IDLE_TIMEOUT_MS + 1000, 4, 2),
        ]);
        assert!(reasons(&output).is_empty());
        assert_eq!(summaries(&output)[0].event_count, 1);
    }

    #[test]
    fn heartbeat_is_still_validated_and_requires_normal_required_fields() {
        let mut heartbeat = event("a", "s", "HEARTBEAT", 0, 1, 1);
        heartbeat.session_id = None;
        assert!(validate(heartbeat.clone()).is_ok());
        heartbeat.power_kw = Some(-0.001);
        assert_eq!(
            validate(heartbeat.clone()).unwrap_err().reason,
            DlqReason::NegativePower
        );
        heartbeat.seq = None;
        assert_eq!(
            validate(heartbeat).unwrap_err().reason,
            DlqReason::MissingField
        );
    }

    #[test]
    fn expiration_precedes_duplicate_or_late_and_uses_last_event_not_deadline() {
        for seq in [1, 2] {
            let output = Processor::new().process_batch([
                event("a", "s", "CHARGING", 0, 1, 1),
                event("b", "s", "COMPLETE", IDLE_TIMEOUT_MS, 2, seq),
            ]);
            assert!(
                matches!(&output[0], Emitted::Summary(s) if s.close_reason == CloseReason::Timeout && s.end_ts == 0)
            );
            assert_eq!(
                reasons(&output),
                [if seq == 1 {
                    DlqReason::Duplicate
                } else {
                    DlqReason::Late
                }]
            );
        }
    }

    #[test]
    fn late_event_can_advance_clock_and_expire_another_session() {
        let output = Processor::new().process_batch([
            event("a", "closed", "COMPLETE", 0, 1, 1),
            event("b", "open", "CHARGING", 0, 2, 2),
            event("c", "closed", "CHARGING", IDLE_TIMEOUT_MS, 3, 3),
        ]);
        assert_eq!(summaries(&output)[1].session_id, "open");
        assert_eq!(summaries(&output)[1].close_reason, CloseReason::Timeout);
        assert_eq!(reasons(&output), [DlqReason::Late]);
    }

    #[test]
    fn concurrent_sessions_use_max_event_time_and_never_another_vehicles_clock() {
        let mut other = event("c", "other", "HEARTBEAT", IDLE_TIMEOUT_MS * 10, 3, 1);
        other.vehicle_id = Some("VH-2".to_owned());
        let output = Processor::new().process_batch([
            event("a", "z", "CHARGING", 1000, 1, 1),
            event("b", "a", "CHARGING", 1000, 2, 2),
            other,
            event("d", "z", "CHARGING", 0, 4, 3),
            event("e", "", "HEARTBEAT", IDLE_TIMEOUT_MS, 5, 4),
            event("f", "", "HEARTBEAT", IDLE_TIMEOUT_MS + 1000, 6, 4),
        ]);
        assert_eq!(
            summaries(&output)
                .iter()
                .map(|s| s.session_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(summaries(&output)[1].event_count, 2);
        assert_eq!(summaries(&output)[1].end_ts, 1000);
    }

    #[test]
    fn clock_never_regresses_and_ingest_time_does_not_trigger_timeout() {
        let mut processor = Processor::new();
        processor.process_batch([event("a", "s", "CHARGING", 100, i64::MAX - 2, 1)]);
        assert!(processor
            .process_batch([event("b", "s", "CHARGING", 0, i64::MAX - 1, 2)])
            .is_empty());
        assert_eq!(processor.vehicles["VH-1"].clock, 100);
        assert_eq!(processor.vehicles["VH-1"].sessions["s"].last_event_ts, 100);
    }

    #[test]
    fn unicode_id_ties_follow_scala_utf16_order_and_exact_ties_are_stable() {
        let mut first = event("\u{10000}", "s", "CHARGING", 0, 1, 1);
        first.soc_pct = Some(10.0);
        let mut second = event("\u{e000}", "s", "CHARGING", 0, 1, 2);
        second.soc_pct = Some(30.0);
        let mut end = event("end", "s", "COMPLETE", 1, 2, 3);
        end.soc_pct = None;
        let output = Processor::new().process_batch([second, first.clone(), end]);
        assert_eq!(summaries(&output)[0].start_soc_pct, Some(10.0));
        assert_eq!(summaries(&output)[0].end_soc_pct, Some(30.0));
        let mut identical = first.clone();
        identical.charging_state = Some("COMPLETE".to_owned());
        let output = Processor::new().process_batch([first, identical]);
        assert_eq!(reasons(&output), [DlqReason::Duplicate]);
        assert!(summaries(&output).is_empty());
    }
}
