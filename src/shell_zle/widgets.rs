//! ZLE Widget implementations

use super::ZleState;

/// Result of executing a widget
#[derive(Debug, Clone)]
pub enum WidgetResult {
    /// Widget executed successfully
    Ok,
    /// Widget needs to call a shell function
    CallFunction(String),
    /// Accept the line (execute command)
    Accept,
    /// Abort/break
    Abort,
    /// Key sequence is incomplete, wait for more input
    Pending,
    /// Key was not handled
    Ignored,
    /// Error occurred
    Error(String),
    /// Refresh display
    Refresh,
    /// Clear screen
    Clear,
}

/// A ZLE widget
#[derive(Debug, Clone)]
pub enum Widget {
    /// Built-in widget
    Builtin(BuiltinWidget),
    /// User-defined widget (shell function name)
    User(String),
}

/// Built-in widget types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinWidget {
    // Movement
    ForwardChar,
    BackwardChar,
    ForwardWord,
    BackwardWord,
    BeginningOfLine,
    EndOfLine,

    // Editing
    SelfInsert,
    DeleteChar,
    BackwardDeleteChar,
    KillLine,
    BackwardKillLine,
    KillWord,
    BackwardKillWord,
    KillWholeLine,
    Yank,
    YankPop,

    // Undo
    Undo,
    Redo,

    // History
    UpLineOrHistory,
    DownLineOrHistory,
    BeginningOfHistory,
    EndOfHistory,
    HistoryIncrementalSearchBackward,
    HistoryIncrementalSearchForward,

    // Completion
    ExpandOrComplete,
    CompleteWord,
    MenuComplete,
    ReverseMenuComplete,

    // Accept/Execute
    AcceptLine,
    AcceptAndHold,
    SendBreak,

    // Misc
    ClearScreen,
    Redisplay,
    TransposeChars,
    TransposeWords,
    CapitalizeWord,
    DownCaseWord,
    UpCaseWord,
    QuotedInsert,
    ViCmdMode,
    ViInsert,
    SetMarkCommand,
    ExchangePointAndMark,
    KillRegion,
    CopyRegionAsKill,
}

/// Execute a builtin widget
pub fn execute_builtin(
    state: &mut ZleState,
    widget: BuiltinWidget,
    key: Option<char>,
) -> WidgetResult {
    state.save_undo();

    match widget {
        // Movement
        BuiltinWidget::ForwardChar => {
            let n = state.numeric_arg.unwrap_or(1).unsigned_abs() as usize;
            let chars: Vec<char> = state.buffer.chars().collect();
            state.cursor = (state.cursor + n).min(chars.len());
            state.numeric_arg = None;
            WidgetResult::Ok
        }
        BuiltinWidget::BackwardChar => {
            let n = state.numeric_arg.unwrap_or(1).unsigned_abs() as usize;
            state.cursor = state.cursor.saturating_sub(n);
            state.numeric_arg = None;
            WidgetResult::Ok
        }
        BuiltinWidget::ForwardWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut pos = state.cursor;
            // Skip non-word chars
            while pos < chars.len() && !chars[pos].is_alphanumeric() {
                pos += 1;
            }
            // Skip word chars
            while pos < chars.len() && chars[pos].is_alphanumeric() {
                pos += 1;
            }
            state.cursor = pos;
            WidgetResult::Ok
        }
        BuiltinWidget::BackwardWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut pos = state.cursor;
            // Skip non-word chars
            while pos > 0 && !chars[pos.saturating_sub(1)].is_alphanumeric() {
                pos -= 1;
            }
            // Skip word chars
            while pos > 0 && chars[pos.saturating_sub(1)].is_alphanumeric() {
                pos -= 1;
            }
            state.cursor = pos;
            WidgetResult::Ok
        }
        BuiltinWidget::BeginningOfLine => {
            state.cursor = 0;
            WidgetResult::Ok
        }
        BuiltinWidget::EndOfLine => {
            state.cursor = state.buffer.chars().count();
            WidgetResult::Ok
        }

        // Editing
        BuiltinWidget::SelfInsert => {
            if let Some(c) = key {
                let chars: Vec<char> = state.buffer.chars().collect();
                let mut new_buffer = String::new();
                for (i, ch) in chars.iter().enumerate() {
                    if i == state.cursor {
                        new_buffer.push(c);
                    }
                    new_buffer.push(*ch);
                }
                if state.cursor >= chars.len() {
                    new_buffer.push(c);
                }
                state.buffer = new_buffer;
                state.cursor += 1;
                WidgetResult::Ok
            } else {
                WidgetResult::Ignored
            }
        }
        BuiltinWidget::DeleteChar => {
            let chars: Vec<char> = state.buffer.chars().collect();
            if state.cursor < chars.len() {
                let mut new_buffer = String::new();
                for (i, ch) in chars.iter().enumerate() {
                    if i != state.cursor {
                        new_buffer.push(*ch);
                    }
                }
                state.buffer = new_buffer;
            }
            WidgetResult::Ok
        }
        BuiltinWidget::BackwardDeleteChar => {
            if state.cursor > 0 {
                let chars: Vec<char> = state.buffer.chars().collect();
                let mut new_buffer = String::new();
                for (i, ch) in chars.iter().enumerate() {
                    if i != state.cursor - 1 {
                        new_buffer.push(*ch);
                    }
                }
                state.buffer = new_buffer;
                state.cursor -= 1;
            }
            WidgetResult::Ok
        }
        BuiltinWidget::KillLine => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let killed: String = chars[state.cursor..].iter().collect();
            state.kill_add(&killed);
            state.buffer = chars[..state.cursor].iter().collect();
            WidgetResult::Ok
        }
        BuiltinWidget::BackwardKillLine => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let killed: String = chars[..state.cursor].iter().collect();
            state.kill_add(&killed);
            state.buffer = chars[state.cursor..].iter().collect();
            state.cursor = 0;
            WidgetResult::Ok
        }
        BuiltinWidget::KillWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut end = state.cursor;
            // Skip non-word chars
            while end < chars.len() && !chars[end].is_alphanumeric() {
                end += 1;
            }
            // Skip word chars
            while end < chars.len() && chars[end].is_alphanumeric() {
                end += 1;
            }
            let killed: String = chars[state.cursor..end].iter().collect();
            state.kill_add(&killed);
            let mut new_buffer: String = chars[..state.cursor].iter().collect();
            new_buffer.push_str(&chars[end..].iter().collect::<String>());
            state.buffer = new_buffer;
            WidgetResult::Ok
        }
        BuiltinWidget::BackwardKillWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut start = state.cursor;
            // Skip non-word chars
            while start > 0 && !chars[start.saturating_sub(1)].is_alphanumeric() {
                start -= 1;
            }
            // Skip word chars
            while start > 0 && chars[start.saturating_sub(1)].is_alphanumeric() {
                start -= 1;
            }
            let killed: String = chars[start..state.cursor].iter().collect();
            state.kill_add(&killed);
            let mut new_buffer: String = chars[..start].iter().collect();
            new_buffer.push_str(&chars[state.cursor..].iter().collect::<String>());
            state.buffer = new_buffer;
            state.cursor = start;
            WidgetResult::Ok
        }
        BuiltinWidget::KillWholeLine => {
            let buffer = state.buffer.clone();
            state.kill_add(&buffer);
            state.buffer.clear();
            state.cursor = 0;
            WidgetResult::Ok
        }
        BuiltinWidget::Yank => {
            if let Some(text) = state.yank() {
                let text = text.to_string();
                let chars: Vec<char> = state.buffer.chars().collect();
                let mut new_buffer: String = chars[..state.cursor].iter().collect();
                new_buffer.push_str(&text);
                new_buffer.push_str(&chars[state.cursor..].iter().collect::<String>());
                state.cursor += text.chars().count();
                state.buffer = new_buffer;
            }
            WidgetResult::Ok
        }
        BuiltinWidget::YankPop => {
            if let Some(text) = state.yank_pop() {
                let text = text.to_string();
                // Would need to track last yank position
                let chars: Vec<char> = state.buffer.chars().collect();
                let mut new_buffer: String = chars[..state.cursor].iter().collect();
                new_buffer.push_str(&text);
                new_buffer.push_str(&chars[state.cursor..].iter().collect::<String>());
                state.cursor += text.chars().count();
                state.buffer = new_buffer;
            }
            WidgetResult::Ok
        }

        // Undo
        BuiltinWidget::Undo => {
            // Pop the undo we just saved at the start
            state.undo_stack.pop();
            state.undo();
            WidgetResult::Ok
        }
        BuiltinWidget::Redo => {
            state.undo_stack.pop();
            state.redo();
            WidgetResult::Ok
        }

        // History (would need history integration)
        BuiltinWidget::UpLineOrHistory => WidgetResult::Ok,
        BuiltinWidget::DownLineOrHistory => WidgetResult::Ok,
        BuiltinWidget::BeginningOfHistory => WidgetResult::Ok,
        BuiltinWidget::EndOfHistory => WidgetResult::Ok,
        BuiltinWidget::HistoryIncrementalSearchBackward => WidgetResult::Ok,
        BuiltinWidget::HistoryIncrementalSearchForward => WidgetResult::Ok,

        // Completion
        BuiltinWidget::ExpandOrComplete => WidgetResult::Ok,
        BuiltinWidget::CompleteWord => WidgetResult::Ok,
        BuiltinWidget::MenuComplete => WidgetResult::Ok,
        BuiltinWidget::ReverseMenuComplete => WidgetResult::Ok,

        // Accept/Execute
        BuiltinWidget::AcceptLine => WidgetResult::Accept,
        BuiltinWidget::AcceptAndHold => WidgetResult::Accept,
        BuiltinWidget::SendBreak => WidgetResult::Abort,

        // Misc
        BuiltinWidget::ClearScreen => WidgetResult::Clear,
        BuiltinWidget::Redisplay => WidgetResult::Refresh,
        BuiltinWidget::TransposeChars => {
            let chars: Vec<char> = state.buffer.chars().collect();
            if state.cursor > 0 && state.cursor < chars.len() {
                let mut new_chars = chars.clone();
                new_chars.swap(state.cursor - 1, state.cursor);
                state.buffer = new_chars.iter().collect();
                state.cursor += 1;
            } else if state.cursor >= 2 && state.cursor == chars.len() {
                let mut new_chars = chars.clone();
                new_chars.swap(state.cursor - 2, state.cursor - 1);
                state.buffer = new_chars.iter().collect();
            }
            WidgetResult::Ok
        }
        BuiltinWidget::TransposeWords => {
            // Complex - would need word boundary detection
            WidgetResult::Ok
        }
        BuiltinWidget::CapitalizeWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut new_buffer = String::new();
            let mut pos = state.cursor;
            let mut first = true;

            // Copy before cursor
            for ch in &chars[..pos] {
                new_buffer.push(*ch);
            }

            // Skip non-word
            while pos < chars.len() && !chars[pos].is_alphanumeric() {
                new_buffer.push(chars[pos]);
                pos += 1;
            }

            // Capitalize first, lowercase rest
            while pos < chars.len() && chars[pos].is_alphanumeric() {
                if first {
                    new_buffer.extend(chars[pos].to_uppercase());
                    first = false;
                } else {
                    new_buffer.extend(chars[pos].to_lowercase());
                }
                pos += 1;
            }

            // Copy rest
            for ch in &chars[pos..] {
                new_buffer.push(*ch);
            }

            state.buffer = new_buffer;
            state.cursor = pos;
            WidgetResult::Ok
        }
        BuiltinWidget::DownCaseWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut new_buffer = String::new();
            let mut pos = state.cursor;

            for ch in &chars[..pos] {
                new_buffer.push(*ch);
            }

            while pos < chars.len() && !chars[pos].is_alphanumeric() {
                new_buffer.push(chars[pos]);
                pos += 1;
            }

            while pos < chars.len() && chars[pos].is_alphanumeric() {
                new_buffer.extend(chars[pos].to_lowercase());
                pos += 1;
            }

            for ch in &chars[pos..] {
                new_buffer.push(*ch);
            }

            state.buffer = new_buffer;
            state.cursor = pos;
            WidgetResult::Ok
        }
        BuiltinWidget::UpCaseWord => {
            let chars: Vec<char> = state.buffer.chars().collect();
            let mut new_buffer = String::new();
            let mut pos = state.cursor;

            for ch in &chars[..pos] {
                new_buffer.push(*ch);
            }

            while pos < chars.len() && !chars[pos].is_alphanumeric() {
                new_buffer.push(chars[pos]);
                pos += 1;
            }

            while pos < chars.len() && chars[pos].is_alphanumeric() {
                new_buffer.extend(chars[pos].to_uppercase());
                pos += 1;
            }

            for ch in &chars[pos..] {
                new_buffer.push(*ch);
            }

            state.buffer = new_buffer;
            state.cursor = pos;
            WidgetResult::Ok
        }
        BuiltinWidget::QuotedInsert => {
            // Next character should be inserted literally
            WidgetResult::Pending
        }
        BuiltinWidget::ViCmdMode => {
            state.vi_cmd_mode = true;
            state.keymap = super::KeymapName::ViCommand;
            WidgetResult::Ok
        }
        BuiltinWidget::ViInsert => {
            state.vi_cmd_mode = false;
            state.keymap = super::KeymapName::ViInsert;
            WidgetResult::Ok
        }
        BuiltinWidget::SetMarkCommand => {
            state.mark = state.cursor;
            state.region_active = true;
            WidgetResult::Ok
        }
        BuiltinWidget::ExchangePointAndMark => {
            std::mem::swap(&mut state.cursor, &mut state.mark);
            WidgetResult::Ok
        }
        BuiltinWidget::KillRegion => {
            if state.region_active {
                let (start, end) = if state.cursor < state.mark {
                    (state.cursor, state.mark)
                } else {
                    (state.mark, state.cursor)
                };
                let chars: Vec<char> = state.buffer.chars().collect();
                let killed: String = chars[start..end].iter().collect();
                state.kill_add(&killed);
                let mut new_buffer: String = chars[..start].iter().collect();
                new_buffer.push_str(&chars[end..].iter().collect::<String>());
                state.buffer = new_buffer;
                state.cursor = start;
                state.region_active = false;
            }
            WidgetResult::Ok
        }
        BuiltinWidget::CopyRegionAsKill => {
            if state.region_active {
                let (start, end) = if state.cursor < state.mark {
                    (state.cursor, state.mark)
                } else {
                    (state.mark, state.cursor)
                };
                let chars: Vec<char> = state.buffer.chars().collect();
                let copied: String = chars[start..end].iter().collect();
                state.kill_add(&copied);
                state.region_active = false;
            }
            WidgetResult::Ok
        }
    }
}
