//! Explicit, bounded selection capture. No clipboard copy or unselected text read.
#[cfg(windows)]
mod platform {
    use crate::services::injection::{self, InjectionTarget};
    use anyhow::{Result, anyhow, bail};
    use std::time::{Duration, Instant};
    use windows::Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Accessibility::{
            CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
            IUIAutomationTextRange, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
            UIA_IsReadOnlyAttributeId, UIA_TextPatternId,
        },
    };

    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct Automation {
        client: IUIAutomation2,
        _com: ComGuard,
    }
    impl Automation {
        fn new() -> Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
                let com = ComGuard;
                let client: IUIAutomation2 =
                    CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
                client.SetConnectionTimeout(200)?;
                client.SetTransactionTimeout(200)?;
                Ok(Self { client, _com: com })
            }
        }
    }
    thread_local! {
        static AUTOMATION: std::result::Result<Automation, String> = Automation::new().map_err(|error| error.to_string());
    }
    fn automation<T>(action: impl FnOnce(&IUIAutomation2) -> Result<T>) -> Result<T> {
        AUTOMATION.with(|automation| match automation {
            Ok(automation) => action(&automation.client),
            Err(error) => Err(anyhow!("Windows selection access is unavailable: {error}")),
        })
    }

    pub struct SelectedText {
        pub text: String,
        pub target: InjectionTarget,
        element: IUIAutomationElement,
        range: IUIAutomationTextRange,
    }

    impl SelectedText {
        pub fn capture() -> Result<Self> {
            let target = injection::capture_foreground_target()
                .ok_or_else(|| anyhow!("Select text in another application first."))?;
            automation(|client| unsafe {
                let started = Instant::now();
                let mut element = client.GetFocusedElement()?;
                // Some WinForms providers report the form while Win32 has
                // already focused its editor. Resolve that owned child directly.
                if element.CurrentNativeWindowHandle()?.0 as isize == target.hwnd {
                    use windows::Win32::UI::WindowsAndMessaging::{
                        GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId,
                    };
                    let mut info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                    };
                    let thread = GetWindowThreadProcessId(HWND(target.hwnd as _), None);
                    if GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.0.is_null() {
                        element = client.ElementFromHandle(info.hwndFocus)?;
                    }
                }
                if element.CurrentIsPassword()?.as_bool() {
                    bail!("Voice editing is unavailable in password fields.");
                }
                let root = client.ElementFromHandle(HWND(target.hwnd as _))?;
                let walker = client.RawViewWalker()?;
                let mut ancestor = element.clone();
                let mut belongs = false;
                for _ in 0..24 {
                    if client.CompareElements(&ancestor, &root)?.as_bool() {
                        belongs = true;
                        break;
                    }
                    if started.elapsed() > Duration::from_millis(500) {
                        break;
                    }
                    let Ok(parent) = walker.GetParentElement(&ancestor) else {
                        break;
                    };
                    ancestor = parent;
                }
                if !belongs {
                    bail!("The focused selection does not belong to the target window.");
                }
                let pattern: IUIAutomationTextPattern = element
                    .GetCurrentPatternAs(UIA_TextPatternId)
                    .map_err(|_| {
                        anyhow!("This control does not expose a readable text selection.")
                    })?;
                let selections = pattern.GetSelection()?;
                if selections.Length()? != 1 {
                    bail!("Select one continuous passage to edit.");
                }
                let range = selections.GetElement(0)?;
                if bool::try_from(&range.GetAttributeValue(UIA_IsReadOnlyAttributeId)?)
                    .unwrap_or(true)
                {
                    bail!(
                        "Select text in an editable control. Read-only passages cannot be replaced."
                    );
                }
                let text = range.GetText(16_385)?.to_string();
                if text.trim().is_empty() {
                    bail!("Select some text before starting voice editing.");
                }
                if text.encode_utf16().count() > 16_384 {
                    bail!("Select no more than 16,384 characters.");
                }
                Ok(Self {
                    text,
                    target,
                    element,
                    range,
                })
            })
        }

        pub fn validate(&self) -> Result<()> {
            let current = Self::capture()?;
            automation(|client| unsafe {
                if current.target.hwnd != self.target.hwnd
                    || current.text != self.text
                    || !client
                        .CompareElements(&self.element, &current.element)?
                        .as_bool()
                    || self.range.CompareEndpoints(
                        TextPatternRangeEndpoint_Start,
                        &current.range,
                        TextPatternRangeEndpoint_Start,
                    )? != 0
                    || self.range.CompareEndpoints(
                        TextPatternRangeEndpoint_End,
                        &current.range,
                        TextPatternRangeEndpoint_End,
                    )? != 0
                {
                    bail!(
                        "The target or selected text changed. The edit was saved to History; nothing was replaced."
                    );
                }
                Ok(())
            })
        }
    }
}
#[cfg(windows)]
pub use platform::SelectedText;

#[cfg(not(windows))]
pub struct SelectedText {
    pub text: String,
    pub target: crate::services::injection::InjectionTarget,
}
#[cfg(not(windows))]
impl SelectedText {
    pub fn capture() -> anyhow::Result<Self> {
        anyhow::bail!("Selected-text voice editing is currently available on Windows.")
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::bail!("Selected-text voice editing is currently available on Windows.")
    }
}
