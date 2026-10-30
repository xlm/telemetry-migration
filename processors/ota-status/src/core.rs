use crate::types::{DlqReason, DlqRecord, OtaEvent, OtaStatus, OtaStatusRecord};

pub fn process(event: &OtaEvent) -> Result<OtaStatusRecord, DlqRecord> {
    let _ = (
        &event.event_id,
        &event.vehicle_id,
        &event.campaign_id,
        &event.status,
        &event.progress_pct,
        &event.event_ts,
        &event.schema_version,
        OtaStatus::Downloading,
        OtaStatus::Installing,
        OtaStatus::Succeeded,
        OtaStatus::Failed,
        DlqReason::MissingField,
        DlqReason::UnknownStatus,
    );
    todo!("Migrate the Scala ota-status Job for the live demo")
}
