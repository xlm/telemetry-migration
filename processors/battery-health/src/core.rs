use crate::types::{BatteryEvent, DlqRecord, HealthRecord};

pub fn process(event: &BatteryEvent) -> Result<HealthRecord, DlqRecord> {
    let _ = (
        &event.event_id,
        &event.vehicle_id,
        &event.battery_id,
        &event.soh_pct,
        &event.event_ts,
        &event.schema_version,
    );
    todo!("Migrate the Scala battery-health Job")
}
