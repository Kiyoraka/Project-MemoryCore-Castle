//! What a scheduled task is, in plain words, for the member deciding on its card. Matched on the
//! task's path and name only (never on words Windows shows in its own language). A task Castle
//! does not know says nothing invented.

use super::look::SENTINEL_TASKS;

/// A known kind of task.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Family {
    pub words: &'static str,
    /// Windows' own (or the Sentinel's own): a note, not a question.
    pub own: bool,
}

/// The family of a task by its full name (path and name), or None.
pub fn task_family(full_name: &str) -> Option<Family> {
    let name = full_name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let family = |words, own| Some(Family { words, own });
    if name.starts_with("\\softlanding\\") {
        family("Windows' own tips and getting-started notifications - Windows makes these for your account by itself and renews them every day; harmless", true)
    } else if name.starts_with(SENTINEL_TASKS) {
        family("one of the Sentinel's own tasks (its watch, its alarm or its hand)", true)
    } else if name.starts_with("\\microsoft\\") {
        family("a task that comes with Windows itself", true)
    } else if has(&["onedrive"]) {
        family("Microsoft OneDrive starting or updating itself", false)
    } else if has(&["microsoftedge", "edgeupdate"]) {
        family("Microsoft Edge keeping itself up to date", false)
    } else if has(&["googleupdate", "googlesystem"]) {
        family("Google keeping Chrome up to date", false)
    } else if has(&["nvidia", "nvtmru", "nvprofile", "nvdriver"]) {
        family("NVIDIA's graphics driver tools", false)
    } else {
        None
    }
}

/// A task's actions for its card: a bare `{CLSID}` (a COM action) reads as the Windows component
/// it runs; a program and its arguments stay as they are.
pub fn action_words(actions: &str) -> String {
    actions
        .split(" | ")
        .map(|a| {
            let a = a.trim();
            if is_clsid(a) { format!("runs a Windows component {a}") } else { a.to_string() }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn is_clsid(text: &str) -> bool {
    let Some(inner) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) else { return false };
    inner.len() == 36 && inner.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_family_says_what_it_is() {
        let soft = task_family("\\SoftLanding\\S-1-5-21-1-2-3-1002\\SoftLandingDeferralTask-{724494fb-1111-2222-3333-444455556666}").unwrap();
        assert!(soft.own && soft.words.starts_with("Windows' own tips"));
        assert!(task_family("\\Microsoft\\Windows\\Defrag\\ScheduledDefrag").unwrap().own);
        assert!(task_family("\\MemoryCore Castle Sentinel\\Tick").unwrap().own);
        let drive = task_family("\\OneDrive Standalone Update Task-S-1-5-21-1-2-3-1002").unwrap();
        assert!(!drive.own && drive.words.contains("OneDrive"));
        assert!(task_family("\\GoogleUpdateTaskMachineUA").unwrap().words.contains("Chrome"));
        assert!(task_family("\\NVIDIA GeForce Experience SelfUpdate_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}").unwrap().words.contains("NVIDIA"));
        assert!(task_family("\\NvTmRu_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}").unwrap().words.contains("NVIDIA"));
    }

    #[test]
    fn an_unknown_task_says_nothing_it_does_not_know() {
        assert_eq!(task_family("\\Kettle Brewer"), None);
    }

    #[test]
    fn a_com_action_reads_as_a_windows_component() {
        assert_eq!(action_words("{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}"), "runs a Windows component {AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        assert_eq!(action_words("C:\\s.exe -x | {AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}"), "C:\\s.exe -x | runs a Windows component {AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        assert_eq!(action_words("C:\\tool.exe {AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}"), "C:\\tool.exe {AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}", "an argument is left as it is");
    }
}
