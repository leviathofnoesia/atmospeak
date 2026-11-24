use std::num::NonZeroU32;

use anyhow::Result;

#[derive(Debug)]
pub(super) struct ClipboardOwnership {
    sequence: Option<NonZeroU32>,
    previous_text: Option<String>,
}

impl ClipboardOwnership {
    pub(super) fn can_restore(&self) -> bool {
        self.sequence.is_some() && self.previous_text.is_some()
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct PendingRestore {
    original_text: Option<String>,
    sequence: Option<NonZeroU32>,
}

#[cfg(any(windows, test))]
impl PendingRestore {
    fn owns(&self, sequence: Option<NonZeroU32>) -> bool {
        self.sequence.is_some() && self.sequence == sequence
    }

    pub(super) fn snapshot(
        &self,
        current_text: Option<String>,
        current_sequence: Option<NonZeroU32>,
    ) -> Option<String> {
        if self.owns(current_sequence) {
            self.original_text.clone()
        } else {
            current_text
        }
    }

    pub(super) fn written(
        &mut self,
        original_text: Option<String>,
        sequence: Option<NonZeroU32>,
        restore_requested: bool,
    ) -> ClipboardOwnership {
        self.sequence = sequence.filter(|_| restore_requested);
        self.original_text = self.sequence.and(original_text.clone());
        ClipboardOwnership {
            sequence,
            previous_text: original_text,
        }
    }

    pub(super) fn retain_if_owned(&mut self, sequence: Option<NonZeroU32>) -> bool {
        if self.owns(sequence) {
            true
        } else {
            *self = Self::default();
            false
        }
    }

    pub(super) fn take_restore(
        &mut self,
        ownership: ClipboardOwnership,
        current_sequence: Option<NonZeroU32>,
    ) -> Option<String> {
        self.retain_if_owned(current_sequence);
        self.abandon(&ownership);
        if ownership.sequence.is_some() && ownership.sequence == current_sequence {
            ownership.previous_text
        } else {
            None
        }
    }

    pub(super) fn abandon(&mut self, ownership: &ClipboardOwnership) {
        if self.owns(ownership.sequence) {
            *self = Self::default();
        }
    }
}

#[cfg(windows)]
fn pending() -> Result<std::sync::MutexGuard<'static, PendingRestore>> {
    use std::sync::{Mutex, OnceLock};
    static PENDING: OnceLock<Mutex<PendingRestore>> = OnceLock::new();
    PENDING
        .get_or_init(|| Mutex::new(PendingRestore::default()))
        .lock()
        .map_err(|_| anyhow::anyhow!("clipboard restoration state is unavailable"))
}

#[cfg(windows)]
fn open() -> Result<clipboard_win::Clipboard> {
    // Match arboard's retry timing; zero-delay retries can repeatedly lose the clipboard lock.
    for attempt in 0..=5 {
        match clipboard_win::Clipboard::new() {
            Ok(clipboard) => return Ok(clipboard),
            Err(error) if attempt == 5 => {
                return Err(anyhow::anyhow!(
                    "failed to open system clipboard: {error:?}"
                ))
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
    unreachable!()
}

pub(super) fn write_transcript_to_clipboard(
    text: &str,
    restore_requested: bool,
) -> Result<ClipboardOwnership> {
    #[cfg(windows)]
    {
        use clipboard_win::{formats::Unicode, get, seq_num, set};
        let mut pending = pending()?;
        let _clipboard = open()?;
        let previous = get::<String, _>(Unicode).ok();
        let previous = pending.snapshot(previous, seq_num());
        if let Err(error) = set(Unicode, text) {
            pending.retain_if_owned(seq_num());
            return Err(anyhow::anyhow!(
                "failed to write transcript to clipboard: {error:?}"
            ));
        }
        // Both the write and its generation are observed while this process holds the clipboard.
        Ok(pending.written(previous, seq_num(), restore_requested))
    }
    #[cfg(not(windows))]
    {
        use anyhow::Context;
        arboard::Clipboard::new()
            .context("failed to open system clipboard")?
            .set_text(text.to_owned())
            .context("failed to write transcript to clipboard")?;
        let _ = restore_requested;
        Ok(ClipboardOwnership {
            sequence: None,
            previous_text: None,
        })
    }
}

pub(super) fn restore_clipboard_if_owned(ownership: ClipboardOwnership) -> bool {
    #[cfg(windows)]
    {
        use clipboard_win::{formats::Unicode, seq_num, set};
        let Ok(mut pending) = pending() else {
            return false;
        };
        let Ok(_clipboard) = open() else {
            pending.abandon(&ownership);
            return false;
        };
        let Some(previous) = pending.take_restore(ownership, seq_num()) else {
            return false;
        };
        // Keep the OS lock across the generation check and restoration.
        set(Unicode, previous).is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = ownership;
        false
    }
}

pub(super) fn abandon_clipboard_ownership(ownership: ClipboardOwnership) {
    #[cfg(windows)]
    if let Ok(mut pending) = pending() {
        pending.abandon(&ownership);
    }
    #[cfg(not(windows))]
    let _ = ownership;
}
