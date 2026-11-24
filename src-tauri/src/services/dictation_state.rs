use crate::models::{DictationPhase, NativeDictationEvent};

#[derive(Default)]
pub struct DictationState {
    generation: u64,
    event: Option<NativeDictationEvent>,
}

impl DictationState {
    pub fn snapshot(&self) -> Option<NativeDictationEvent> { self.event.clone() }

    pub fn record(&mut self, mut event: NativeDictationEvent) -> NativeDictationEvent {
        self.generation += 1;
        if matches!(event.phase, DictationPhase::Idle) {
            if let Some(previous) = self.event.as_ref().filter(|previous| matches!(previous.phase, DictationPhase::Error)) {
                event = previous.clone();
            }
        }
        event.generation = self.generation;
        self.event = Some(event.clone());
        event
    }

    pub fn dismiss(&mut self, generation: u64) -> Option<NativeDictationEvent> {
        let mut event = self.event.clone()?;
        if event.generation != generation || !matches!(event.phase, DictationPhase::Error) { return None; }
        self.generation += 1;
        event.generation = self.generation;
        event.phase = DictationPhase::Idle;
        event.recording = None;
        event.message.clear();
        event.result = None;
        self.event = Some(event.clone());
        Some(event)
    }
}
