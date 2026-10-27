use crate::types::{ChargingEvent, Emitted};

/// The Charging-session Processor.
///
/// Events arrive in file order. The Scala Job under `jobs/charging-session/`
/// is the specification.
#[derive(Debug, Default)]
pub struct Processor {}

impl Processor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, ev: ChargingEvent) -> Vec<Emitted> {
        let _ = ev;
        todo!()
    }
}
