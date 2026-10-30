use std::collections::{BTreeMap, HashMap, HashSet};

use crate::types::{
    ChargingEvent, ChargingState, ChargingSummary, CloseReason, DlqReason, DlqRecord, Emitted,
};

const SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Default)]
pub struct Processor {
    vehicles: HashMap<String, VehicleState>,
}

#[derive(Debug)]
struct VehicleState {
    clock: i64,
    open_sessions: BTreeMap<String, Session>,
    closed_session_ids: HashSet<String>,
    seen_dedup_keys: HashSet<(String, i64)>,
}

impl Default for VehicleState {
    fn default() -> Self {
        Self {
            clock: i64::MIN,
            open_sessions: BTreeMap::new(),
            closed_session_ids: HashSet::new(),
            seen_dedup_keys: HashSet::new(),
        }
    }
}

#[derive(Debug)]
struct Session {
    events: Vec<ChargingEvent>,
    last_event_ts: i64,
}

impl Processor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, ev: ChargingEvent) -> Vec<Emitted> {
        let state_kind = match validate(&ev) {
            Ok(state) => state,
            Err(reason) => {
                return vec![Emitted::Dlq(DlqRecord {
                    event_id: ev.event_id,
                    vehicle_id: ev.vehicle_id,
                    reason,
                })]
            }
        };

        let vehicle_id = ev
            .vehicle_id
            .as_ref()
            .expect("validated vehicle id")
            .clone();
        let event_ts = ev.event_ts.expect("validated event timestamp");
        let seq = ev.seq.expect("validated sequence");
        let mut output = Vec::new();
        let vehicle = self.vehicles.entry(vehicle_id.clone()).or_default();

        vehicle.clock = vehicle.clock.max(event_ts);
        let expired = vehicle
            .open_sessions
            .iter()
            .filter(|(_, session)| {
                session.last_event_ts.wrapping_add(SESSION_IDLE_TIMEOUT_MS) <= vehicle.clock
            })
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();

        for session_id in expired {
            if let Some(session) = vehicle.open_sessions.remove(&session_id) {
                vehicle.closed_session_ids.insert(session_id.clone());
                output.push(Emitted::Summary(build_summary(
                    &session_id,
                    &vehicle_id,
                    &session,
                    CloseReason::Timeout,
                )));
            }
        }

        if state_kind == ChargingState::Heartbeat {
            return output;
        }

        let dedup_key = (vehicle_id.clone(), seq);
        if vehicle.seen_dedup_keys.contains(&dedup_key) {
            output.push(Emitted::Dlq(DlqRecord {
                event_id: ev.event_id,
                vehicle_id: ev.vehicle_id,
                reason: DlqReason::Duplicate,
            }));
            return output;
        }

        let session_id = ev
            .session_id
            .as_ref()
            .expect("validated session id")
            .clone();
        if vehicle.closed_session_ids.contains(&session_id) {
            output.push(Emitted::Dlq(DlqRecord {
                event_id: ev.event_id,
                vehicle_id: ev.vehicle_id,
                reason: DlqReason::Late,
            }));
            return output;
        }

        let session = vehicle
            .open_sessions
            .entry(session_id.clone())
            .or_insert_with(|| Session {
                events: Vec::new(),
                last_event_ts: event_ts,
            });
        session.last_event_ts = session.last_event_ts.max(event_ts);
        session.events.push(ev);
        vehicle.seen_dedup_keys.insert(dedup_key);

        if matches!(
            state_kind,
            ChargingState::Complete | ChargingState::Unplugged
        ) {
            let session = vehicle
                .open_sessions
                .remove(&session_id)
                .expect("session was just inserted");
            vehicle.closed_session_ids.insert(session_id.clone());
            let close_reason = if state_kind == ChargingState::Complete {
                CloseReason::Complete
            } else {
                CloseReason::Unplugged
            };
            output.push(Emitted::Summary(build_summary(
                &session_id,
                &vehicle_id,
                &session,
                close_reason,
            )));
        }

        output
    }
}

fn validate(event: &ChargingEvent) -> Result<ChargingState, DlqReason> {
    if event.event_id.is_none()
        || event.vehicle_id.is_none()
        || event.event_ts.is_none()
        || event.charging_state.is_none()
        || event.ingest_ts.is_none()
        || event.seq.is_none()
    {
        return Err(DlqReason::MissingField);
    }

    let state = event
        .charging_state
        .as_deref()
        .and_then(ChargingState::from_wire)
        .ok_or(DlqReason::UnknownState)?;

    if event.session_id.is_none() && state != ChargingState::Heartbeat {
        return Err(DlqReason::MissingField);
    }

    if event.soc_pct.is_some_and(|soc| soc < 0.0 || soc > 100.0) {
        return Err(DlqReason::SocOutOfRange);
    }

    if event.power_kw.is_some_and(|power| power < 0.0) {
        return Err(DlqReason::NegativePower);
    }

    Ok(state)
}

fn build_summary(
    session_id: &str,
    vehicle_id: &str,
    session: &Session,
    close_reason: CloseReason,
) -> ChargingSummary {
    let mut events = session.events.iter().collect::<Vec<_>>();
    events.sort_by(|left, right| {
        left.event_ts
            .cmp(&right.event_ts)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });

    let first = events.first().expect("sessions contain accepted events");
    let last = events.last().expect("sessions contain accepted events");
    let start_ts = first.event_ts.expect("accepted event has a timestamp");
    let end_ts = last.event_ts.expect("accepted event has a timestamp");
    let energy = events.windows(2).fold(0.0, |total, pair| {
        let first = pair[0];
        let second = pair[1];
        if first.charging_state.as_deref() == Some("CHARGING") {
            let first_power = first.power_kw.unwrap_or(0.0);
            let second_power = second.power_kw.unwrap_or(0.0);
            total
                + 0.5
                    * (first_power + second_power)
                    * (second
                        .event_ts
                        .expect("accepted event has a timestamp")
                        .wrapping_sub(first.event_ts.expect("accepted event has a timestamp"))
                        as f64)
                    / 3_600_000.0
        } else {
            total
        }
    });
    let peak_power_kw = events
        .iter()
        .filter_map(|event| event.power_kw)
        .fold(0.0, |peak, power| if power > peak { power } else { peak });

    ChargingSummary {
        session_id: session_id.to_owned(),
        vehicle_id: vehicle_id.to_owned(),
        start_ts,
        end_ts,
        duration_s: (end_ts.wrapping_sub(start_ts) / 1000) as i32 as i64,
        start_soc_pct: events.iter().find_map(|event| event.soc_pct),
        end_soc_pct: events.iter().rev().find_map(|event| event.soc_pct),
        energy_kwh: round_half_even_3(energy),
        peak_power_kw,
        event_count: events.len() as u64,
        close_reason,
        schema_version: "1".to_owned(),
    }
}

fn round_half_even_3(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }

    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("scientific float has an exponent");
    let exponent = exponent.parse::<i32>().expect("float exponent is numeric");
    let digits = mantissa
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect::<String>();
    let retained = exponent + 4;

    if retained <= 0 {
        if exponent == -4 {
            let remainder = &digits[1..];
            let first = digits.as_bytes()[0] - b'0';
            let rounds_up = first > 5 || (first == 5 && remainder.bytes().any(|b| b != b'0'));
            if rounds_up {
                return if value.is_sign_negative() {
                    -0.001
                } else {
                    0.001
                };
            }
        }
        return 0.0;
    }

    let retained = retained as usize;
    let round_up = if retained < digits.len() {
        let discarded = &digits[retained..];
        let first_discarded = discarded.as_bytes()[0] - b'0';
        first_discarded > 5
            || (first_discarded == 5
                && (discarded[1..].bytes().any(|digit| digit != b'0')
                    || (digits.as_bytes()[retained - 1] - b'0') % 2 == 1))
    } else {
        false
    };

    let mut scaled = if retained < digits.len() {
        digits[..retained].to_owned()
    } else {
        format!("{}{}", digits, "0".repeat(retained - digits.len()))
    };
    if round_up {
        increment_decimal(&mut scaled);
    }
    while scaled.len() < 4 {
        scaled.insert(0, '0');
    }

    let decimal_point = scaled.len() - 3;
    let sign = if value.is_sign_negative() { "-" } else { "" };
    format!(
        "{sign}{}.{}",
        &scaled[..decimal_point],
        &scaled[decimal_point..]
    )
    .parse()
    .expect("rounded decimal is a finite float")
}

fn increment_decimal(digits: &mut String) {
    let mut bytes = digits.as_bytes().to_vec();
    for index in (0..bytes.len()).rev() {
        if bytes[index] == b'9' {
            bytes[index] = b'0';
        } else {
            bytes[index] += 1;
            *digits = String::from_utf8(bytes).expect("decimal digits are valid UTF-8");
            return;
        }
    }
    bytes.insert(0, b'1');
    *digits = String::from_utf8(bytes).expect("decimal digits are valid UTF-8");
}

#[cfg(test)]
mod tests {
    use super::{round_half_even_3, Processor, SESSION_IDLE_TIMEOUT_MS};
    use crate::types::{ChargingEvent, ChargingSummary, CloseReason, DlqReason, Emitted};

    #[allow(clippy::too_many_arguments)]
    fn event(
        id: &str,
        vehicle: &str,
        session: Option<&str>,
        state: &str,
        event_ts: i64,
        seq: i64,
        soc: Option<f64>,
        power: Option<f64>,
    ) -> ChargingEvent {
        ChargingEvent {
            event_id: Some(id.to_owned()),
            vehicle_id: Some(vehicle.to_owned()),
            session_id: session.map(str::to_owned),
            event_ts: Some(event_ts),
            ingest_ts: Some(event_ts),
            seq: Some(seq),
            charging_state: Some(state.to_owned()),
            soc_pct: soc,
            power_kw: power,
            location: None,
            schema_version: None,
        }
    }

    fn valid_event() -> ChargingEvent {
        event(
            "event-1",
            "vehicle-1",
            Some("session-1"),
            "CHARGING",
            1,
            1,
            Some(50.0),
            Some(10.0),
        )
    }

    fn dlq(output: &[Emitted]) -> Vec<(Option<String>, Option<String>, DlqReason)> {
        output
            .iter()
            .filter_map(|emission| match emission {
                Emitted::Dlq(record) => Some((
                    record.event_id.clone(),
                    record.vehicle_id.clone(),
                    record.reason,
                )),
                Emitted::Summary(_) => None,
            })
            .collect()
    }

    fn summaries(output: &[Emitted]) -> Vec<&ChargingSummary> {
        output
            .iter()
            .filter_map(|emission| match emission {
                Emitted::Summary(summary) => Some(summary),
                Emitted::Dlq(_) => None,
            })
            .collect()
    }

    fn parse_or_default(line: &str) -> ChargingEvent {
        serde_json::from_str(line).unwrap_or_default()
    }

    #[test]
    fn validation_rejects_each_missing_required_field_with_first_reason() {
        let mut cases = Vec::new();
        let mut missing = valid_event();
        missing.event_id = None;
        cases.push(missing);
        let mut missing = valid_event();
        missing.vehicle_id = None;
        cases.push(missing);
        let mut missing = valid_event();
        missing.event_ts = None;
        cases.push(missing);
        let mut missing = valid_event();
        missing.charging_state = None;
        cases.push(missing);
        let mut missing = valid_event();
        missing.ingest_ts = None;
        cases.push(missing);
        let mut missing = valid_event();
        missing.seq = None;
        cases.push(missing);

        for event in cases {
            let mut processor = Processor::new();
            let expected_event_id = event.event_id.clone();
            let expected_vehicle_id = event.vehicle_id.clone();
            assert_eq!(
                dlq(&processor.on_event(event)),
                vec![(
                    expected_event_id,
                    expected_vehicle_id,
                    DlqReason::MissingField
                )]
            );
        }
    }

    #[test]
    fn validation_precedence_and_dlq_identity_match_the_job() {
        let mut processor = Processor::new();
        let mut missing = valid_event();
        missing.event_id = None;
        missing.charging_state = Some("INVALID".to_owned());
        missing.soc_pct = Some(101.0);
        missing.power_kw = Some(-1.0);
        assert_eq!(
            dlq(&processor.on_event(missing)),
            vec![(None, Some("vehicle-1".to_owned()), DlqReason::MissingField)]
        );

        let mut unknown = valid_event();
        unknown.charging_state = Some("charging".to_owned());
        unknown.session_id = None;
        unknown.soc_pct = Some(101.0);
        unknown.power_kw = Some(-1.0);
        assert_eq!(
            dlq(&processor.on_event(unknown)),
            vec![(
                Some("event-1".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::UnknownState
            )]
        );

        let mut missing_session = valid_event();
        missing_session.session_id = None;
        missing_session.soc_pct = Some(101.0);
        missing_session.power_kw = Some(-1.0);
        assert_eq!(
            dlq(&processor.on_event(missing_session)),
            vec![(
                Some("event-1".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::MissingField
            )]
        );

        let mut out_of_range = valid_event();
        out_of_range.soc_pct = Some(-0.01);
        out_of_range.power_kw = Some(-1.0);
        assert_eq!(
            dlq(&processor.on_event(out_of_range)),
            vec![(
                Some("event-1".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::SocOutOfRange
            )]
        );

        let mut negative_power = valid_event();
        negative_power.power_kw = Some(-1.0);
        assert_eq!(
            dlq(&processor.on_event(negative_power)),
            vec![(
                Some("event-1".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::NegativePower
            )]
        );
    }

    #[test]
    fn validation_accepts_soc_bounds_and_negative_zero_power() {
        let mut processor = Processor::new();
        assert!(processor
            .on_event(event(
                "low",
                "vehicle-1",
                Some("low-session"),
                "COMPLETE",
                1,
                1,
                Some(0.0),
                Some(-0.0)
            ))
            .iter()
            .all(|emission| matches!(emission, Emitted::Summary(_))));
        assert!(processor
            .on_event(event(
                "high",
                "vehicle-1",
                Some("high-session"),
                "COMPLETE",
                2,
                2,
                Some(100.0),
                None
            ))
            .iter()
            .all(|emission| matches!(emission, Emitted::Summary(_))));
    }

    #[test]
    fn heartbeat_without_session_advances_clock_and_times_out_open_sessions() {
        let mut processor = Processor::new();
        assert!(processor
            .on_event(event(
                "open",
                "vehicle-1",
                Some("session-1"),
                "CHARGING",
                0,
                1,
                Some(20.0),
                Some(3.0)
            ))
            .is_empty());

        let output = processor.on_event(event(
            "heartbeat",
            "vehicle-1",
            None,
            "HEARTBEAT",
            SESSION_IDLE_TIMEOUT_MS,
            1,
            Some(30.0),
            None,
        ));
        let summaries = summaries(&output);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].close_reason, CloseReason::Timeout);
        assert_eq!(summaries[0].event_count, 1);
    }

    #[test]
    fn heartbeat_soc_is_validated_and_heartbeats_are_not_deduplicated() {
        let mut processor = Processor::new();
        let mut invalid = event(
            "bad-heartbeat",
            "vehicle-1",
            None,
            "HEARTBEAT",
            1,
            1,
            Some(101.0),
            None,
        );
        assert_eq!(
            dlq(&processor.on_event(invalid.clone())),
            vec![(
                Some("bad-heartbeat".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::SocOutOfRange
            )]
        );
        invalid.event_id = Some("heartbeat".to_owned());
        invalid.soc_pct = None;
        assert!(processor.on_event(invalid).is_empty());

        let accepted = event(
            "accepted",
            "vehicle-1",
            Some("session-1"),
            "COMPLETE",
            2,
            1,
            None,
            None,
        );
        assert!(summaries(&processor.on_event(accepted)).len() == 1);
    }

    #[test]
    fn timeout_uses_the_event_clock_and_includes_the_exact_boundary() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "open",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            0,
            1,
            None,
            None,
        ));

        let mut before = event(
            "before",
            "vehicle-1",
            None,
            "HEARTBEAT",
            SESSION_IDLE_TIMEOUT_MS - 1,
            2,
            None,
            None,
        );
        before.ingest_ts = Some(i64::MAX);
        assert!(summaries(&processor.on_event(before)).is_empty());

        let mut at_boundary = event(
            "boundary",
            "vehicle-1",
            None,
            "HEARTBEAT",
            SESSION_IDLE_TIMEOUT_MS,
            2,
            None,
            None,
        );
        at_boundary.ingest_ts = Some(i64::MIN);
        let output = processor.on_event(at_boundary);
        assert_eq!(summaries(&output)[0].close_reason, CloseReason::Timeout);
    }

    #[test]
    fn clock_never_moves_back_and_old_sessions_expire_on_the_next_event() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "clock",
            "vehicle-1",
            None,
            "HEARTBEAT",
            10_000_000,
            1,
            None,
            None,
        ));
        assert!(processor
            .on_event(event(
                "old-open",
                "vehicle-1",
                Some("old-session"),
                "CHARGING",
                0,
                2,
                None,
                None
            ))
            .is_empty());

        let output = processor.on_event(event(
            "next",
            "vehicle-1",
            Some("new-session"),
            "CHARGING",
            1,
            3,
            None,
            None,
        ));
        assert_eq!(summaries(&output)[0].session_id, "old-session");
        assert_eq!(summaries(&output)[0].close_reason, CloseReason::Timeout);
    }

    #[test]
    fn timeouts_precede_the_current_event_dlq_and_use_sorted_session_ids() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "open-z",
            "vehicle-1",
            Some("z-session"),
            "CHARGING",
            0,
            1,
            None,
            None,
        ));
        processor.on_event(event(
            "open-a",
            "vehicle-1",
            Some("a-session"),
            "CHARGING",
            0,
            2,
            None,
            None,
        ));

        let output = processor.on_event(event(
            "duplicate",
            "vehicle-1",
            Some("other-session"),
            "CHARGING",
            SESSION_IDLE_TIMEOUT_MS,
            1,
            None,
            None,
        ));
        assert_eq!(output.len(), 3);
        assert!(matches!(
            &output[0],
            Emitted::Summary(summary) if summary.session_id == "a-session"
        ));
        assert!(matches!(
            &output[1],
            Emitted::Summary(summary) if summary.session_id == "z-session"
        ));
        assert_eq!(dlq(&output)[0].2, DlqReason::Duplicate);
    }

    #[test]
    fn deduplication_spans_sessions_and_uses_vehicle_and_sequence() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "first",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            1,
            7,
            None,
            None,
        ));

        let duplicate = processor.on_event(event(
            "new-event-id",
            "vehicle-1",
            Some("session-b"),
            "CHARGING",
            2,
            7,
            None,
            None,
        ));
        assert_eq!(dlq(&duplicate)[0].2, DlqReason::Duplicate);
        assert_eq!(processor.vehicles["vehicle-1"].seen_dedup_keys.len(), 1);
    }

    #[test]
    fn closed_sessions_reject_late_events_without_recording_their_keys() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "close",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            1,
            1,
            None,
            None,
        ));

        let late = processor.on_event(event(
            "late",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            2,
            2,
            None,
            None,
        ));
        assert_eq!(dlq(&late)[0].2, DlqReason::Late);
        assert_eq!(processor.vehicles["vehicle-1"].seen_dedup_keys.len(), 1);

        let accepted = processor.on_event(event(
            "same-sequence-new-session",
            "vehicle-1",
            Some("session-b"),
            "COMPLETE",
            3,
            2,
            None,
            None,
        ));
        assert_eq!(summaries(&accepted).len(), 1);
        assert_eq!(processor.vehicles["vehicle-1"].seen_dedup_keys.len(), 2);
    }

    #[test]
    fn duplicate_check_precedes_late_check() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "close",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            1,
            1,
            None,
            None,
        ));
        let output = processor.on_event(event(
            "duplicate-late",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            2,
            1,
            None,
            None,
        ));

        assert_eq!(dlq(&output)[0].2, DlqReason::Duplicate);
    }

    #[test]
    fn out_of_order_session_events_are_sorted_for_summary_and_energy() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "b",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            60_000,
            1,
            Some(21.0),
            Some(74.07),
        ));
        processor.on_event(event(
            "a",
            "vehicle-1",
            Some("session-a"),
            "PLUGGED_IN",
            0,
            2,
            Some(20.0),
            None,
        ));
        let output = processor.on_event(event(
            "c",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            120_000,
            3,
            Some(22.0),
            Some(74.07),
        ));
        let summary = summaries(&output)[0];

        assert_eq!(summary.start_ts, 0);
        assert_eq!(summary.end_ts, 120_000);
        assert_eq!(summary.start_soc_pct, Some(20.0));
        assert_eq!(summary.end_soc_pct, Some(22.0));
        assert_eq!(summary.energy_kwh, 1.234);
        assert_eq!(summary.event_count, 3);
    }

    #[test]
    fn open_sessions_are_not_flushed_at_end_of_input() {
        let mut processor = Processor::new();

        assert!(processor
            .on_event(event(
                "open",
                "vehicle-1",
                Some("session-a"),
                "CHARGING",
                1,
                1,
                None,
                None
            ))
            .is_empty());
        assert_eq!(processor.vehicles["vehicle-1"].open_sessions.len(), 1);
    }

    #[test]
    fn vehicles_have_independent_sessions_and_deduplication() {
        let mut processor = Processor::new();
        let one = processor.on_event(event(
            "one",
            "vehicle-1",
            Some("same-session"),
            "COMPLETE",
            1,
            1,
            None,
            None,
        ));
        let two = processor.on_event(event(
            "two",
            "vehicle-2",
            Some("same-session"),
            "COMPLETE",
            1,
            1,
            None,
            None,
        ));

        assert_eq!(summaries(&one)[0].vehicle_id, "vehicle-1");
        assert_eq!(summaries(&two)[0].vehicle_id, "vehicle-2");
    }

    #[test]
    fn energy_counts_charging_trapezoids_and_treats_missing_power_as_zero() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "a",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            0,
            1,
            None,
            Some(120.0),
        ));
        processor.on_event(event(
            "b",
            "vehicle-1",
            Some("session-a"),
            "PAUSED",
            60_000,
            2,
            None,
            None,
        ));
        processor.on_event(event(
            "c",
            "vehicle-1",
            Some("session-a"),
            "PAUSED",
            120_000,
            3,
            None,
            Some(50.0),
        ));
        let output = processor.on_event(event(
            "d",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            180_000,
            4,
            None,
            None,
        ));
        let summary = summaries(&output)[0];

        assert_eq!(summary.energy_kwh, 1.0);
        assert_eq!(summary.peak_power_kw, 120.0);
    }

    #[test]
    fn absent_power_and_soc_have_contract_defaults() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "a",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            1,
            1,
            None,
            None,
        ));
        let output = processor.on_event(event(
            "b",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            2,
            2,
            None,
            None,
        ));
        let summary = summaries(&output)[0];

        assert_eq!(summary.peak_power_kw, 0.0);
        assert_eq!(summary.peak_power_kw.to_bits(), 0.0f64.to_bits());
        assert_eq!(summary.start_soc_pct, None);
        assert_eq!(summary.end_soc_pct, None);
    }

    #[test]
    fn summary_uses_int_narrowing_for_duration_and_literal_schema_version() {
        let summary = super::build_summary(
            "session-a",
            "vehicle-1",
            &super::Session {
                events: vec![
                    event(
                        "a",
                        "vehicle-1",
                        Some("session-a"),
                        "CHARGING",
                        0,
                        1,
                        None,
                        None,
                    ),
                    event(
                        "b",
                        "vehicle-1",
                        Some("session-a"),
                        "COMPLETE",
                        i64::MAX,
                        2,
                        None,
                        None,
                    ),
                ],
                last_event_ts: i64::MAX,
            },
            CloseReason::Complete,
        );

        assert_eq!(summary.duration_s, ((i64::MAX / 1000) as i32) as i64);
        assert_eq!(summary.schema_version, "1");
    }

    #[test]
    fn half_even_rounding_matches_the_scala_decimal_examples() {
        for (value, expected) in [
            (0.6625, 0.662),
            (0.6635, 0.664),
            (0.66251, 0.663),
            (1.2345, 1.234),
            (2.0005, 2.0),
            (0.0, 0.0),
            (1e-7, 0.0),
            (12345.6789, 12345.679),
            (-0.6625, -0.662),
        ] {
            assert_eq!(round_half_even_3(value), expected, "{value}");
        }
    }

    #[test]
    fn scala_energy_validation_and_parsing_specs_are_ported() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "a",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            0,
            1,
            Some(20.0),
            Some(74.07),
        ));
        let output = processor.on_event(event(
            "b",
            "vehicle-1",
            Some("session-a"),
            "COMPLETE",
            60_000,
            2,
            Some(21.0),
            Some(74.07),
        ));
        let summary = summaries(&output)[0];

        assert_eq!(summary.energy_kwh, 1.234);
        assert_eq!(summary.duration_s, 60);
        assert_eq!(summary.start_soc_pct, Some(20.0));
        assert_eq!(summary.end_soc_pct, Some(21.0));

        let mut processor = Processor::new();
        for (state, soc, power, expected) in [
            ("INVALID", Some(-1.0), Some(-1.0), DlqReason::UnknownState),
            (
                "CHARGING",
                Some(101.0),
                Some(-1.0),
                DlqReason::SocOutOfRange,
            ),
            ("CHARGING", Some(50.0), Some(-1.0), DlqReason::NegativePower),
        ] {
            let mut event = valid_event();
            event.charging_state = Some(state.to_owned());
            event.soc_pct = soc;
            event.power_kw = power;
            assert_eq!(dlq(&processor.on_event(event))[0].2, expected);
        }

        let corrupt = parse_or_default(r#"{"event_id":""#);
        assert_eq!(
            dlq(&processor.on_event(corrupt)),
            vec![(None, None, DlqReason::MissingField)]
        );
    }

    #[test]
    fn duplicate_sequence_and_later_event_time_close_idle_session() {
        let mut processor = Processor::new();
        processor.on_event(event(
            "a",
            "vehicle-1",
            Some("session-a"),
            "PLUGGED_IN",
            1_000,
            1,
            Some(0.0),
            None,
        ));
        let duplicate = processor.on_event(event(
            "duplicate",
            "vehicle-1",
            Some("session-a"),
            "CHARGING",
            1_001,
            1,
            Some(20.0),
            Some(10.0),
        ));
        let close_idle = processor.on_event(event(
            "b",
            "vehicle-1",
            Some("session-b"),
            "PLUGGED_IN",
            1_801_000,
            2,
            Some(0.0),
            None,
        ));

        assert_eq!(dlq(&duplicate)[0].2, DlqReason::Duplicate);
        assert_eq!(summaries(&close_idle)[0].session_id, "session-a");
        assert_eq!(summaries(&close_idle)[0].close_reason, CloseReason::Timeout);
    }

    #[test]
    fn jsonl_deserialization_cases_reach_the_processor_as_specified() {
        let earlier_wrong_later_valid = parse_or_default(
            r#"{"event_id":7,"event_id":"good","vehicle_id":"vehicle-1","session_id":"session-a","event_ts":1,"ingest_ts":1,"seq":1,"charging_state":"COMPLETE"}"#,
        );
        let mut processor = Processor::new();
        assert_eq!(
            summaries(&processor.on_event(earlier_wrong_later_valid)).len(),
            1
        );

        let earlier_valid_later_wrong = parse_or_default(
            r#"{"event_id":"bad","event_id":7,"vehicle_id":"vehicle-1","session_id":"session-b","event_ts":2,"ingest_ts":2,"seq":2,"charging_state":"COMPLETE"}"#,
        );
        assert_eq!(
            dlq(&processor.on_event(earlier_valid_later_wrong)),
            vec![(None, Some("vehicle-1".to_owned()), DlqReason::MissingField)]
        );

        for input in ["[]", r#""text""#, "not json"] {
            let event = parse_or_default(input);
            assert_eq!(
                dlq(&processor.on_event(event)),
                vec![(None, None, DlqReason::MissingField)]
            );
        }

        let wrong_soc = parse_or_default(
            r#"{"event_id":"soc","vehicle_id":"vehicle-1","session_id":"session-c","event_ts":3,"ingest_ts":3,"seq":3,"charging_state":"COMPLETE","soc_pct":"25"}"#,
        );
        let output = processor.on_event(wrong_soc);
        assert_eq!(summaries(&output)[0].start_soc_pct, None);

        let wrong_seq = parse_or_default(
            r#"{"event_id":"seq","vehicle_id":"vehicle-1","session_id":"session-d","event_ts":4,"ingest_ts":4,"seq":1.0,"charging_state":"COMPLETE"}"#,
        );
        assert_eq!(
            dlq(&processor.on_event(wrong_seq)),
            vec![(
                Some("seq".to_owned()),
                Some("vehicle-1".to_owned()),
                DlqReason::MissingField
            )]
        );

        let known_overflow =
            serde_json::from_str::<ChargingEvent>(r#"{"soc_pct":1e400}"#).unwrap_or_default();
        assert_eq!(
            dlq(&processor.on_event(known_overflow)),
            vec![(None, None, DlqReason::MissingField)]
        );
        let unknown_overflow = parse_or_default(
            r#"{"event_id":"overflow","vehicle_id":"vehicle-1","session_id":"session-e","event_ts":5,"ingest_ts":5,"seq":5,"charging_state":"COMPLETE","extra":1e400}"#,
        );
        assert_eq!(summaries(&processor.on_event(unknown_overflow)).len(), 1);
    }
}
