#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteMode {
    Terminal,
    Text,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PasteTarget {
    pub is_purpose_terminal: bool,
    pub is_terminal_window: bool,
}

pub fn snapshot_paste_target(is_purpose_terminal: bool, is_terminal_window: bool) -> PasteTarget {
    PasteTarget {
        is_purpose_terminal,
        is_terminal_window,
    }
}

pub fn decide_paste_mode(target: Option<&PasteTarget>, is_live_terminal: bool) -> PasteMode {
    if target.is_some_and(|target| target.is_terminal_window || target.is_purpose_terminal)
        || is_live_terminal
    {
        PasteMode::Terminal
    } else {
        PasteMode::Text
    }
}

pub fn is_terminal_window<T: AsRef<str>>(
    wm_class: Option<&str>,
    app_id: Option<&str>,
    ids: &[T],
) -> bool {
    let terminals: Vec<String> = ids
        .iter()
        .map(|id| id.as_ref().trim())
        .filter(|id| !id.is_empty())
        .map(|id| id.to_lowercase())
        .collect();

    [wm_class, app_id]
        .into_iter()
        .flatten()
        .any(|id| terminals.contains(&id.to_lowercase()))
}
