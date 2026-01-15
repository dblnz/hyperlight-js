//! Debugger module for QuickJS guest
//!
//! This module provides debugging capabilities by tracking execution state,
//! managing breakpoints, and communicating with the host debugger via the
//! `hl_dap_debug_break` host function.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};
use core::cell::OnceCell;

use anyhow::Result;
use hashbrown::HashSet;
use hyperlight_common::flatbuffer_wrappers::function_types::{ParameterValue, ReturnType};
use hyperlight_guest_bin::host_comm::call_host_function;
use rquickjs::{Context, Ctx};

/// Host function name for debug break communication
const DEBUG_BREAK_FUNC_NAME: &str = "hl_dap_debug_break";

pub struct Debugger;

impl hyperlight_js_runtime::debugger::Debugger for Debugger {
    fn enable_debugging(&self, ctx: &Context) -> Result<()> {
        ctx.runtime().set_debug_trace_handler(
            &ctx,
            Some(alloc::boxed::Box::new(
                |context, filename, funcname, line, col| {
                    debug_trace_handler(&context, filename, funcname, line, col)
                },
            )),
        );

        Ok(())
    }
}

/// Debugger execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugMode {
    /// Normal execution, only stop at breakpoints
    Running,
    /// Stop at the next operation (step into)
    Step,
    /// Stop at the next operation in the same or parent frame (step over)
    StepOver,
    /// Stop when returning to the parent frame (step out)
    StepOut,
    /// Debugger is paused (should not happen during execution)
    Paused,
}

/// Variable information
#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: String,
}

/// A single stack frame
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub id: u32,
    pub name: String,
    pub location: Location,
    pub variables: Vec<Variable>,
}

impl StackFrame {
    fn to_json(&self) -> String {
        format!(
            r#"{{"id":{},"name":"{}","location":{}}}"#,
            self.id,
            escape_json_string(&self.name),
            self.location.to_json(),
        )
    }
}

/// A breakpoint identified by file and line
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Breakpoint {
    pub filename: String,
    pub line: i32,
}

/// The reason why execution stopped
#[derive(Debug, Clone, Copy)]
pub enum StopReason {
    Entry,
    /// Hit a breakpoint
    Breakpoint,
    /// Completed a step operation
    Step,
    /// Explicit debugger statement
    Pause,
    /// Excution hit a debugger statement
    Exception,
}

impl StopReason {
    fn as_str(&self) -> &'static str {
        match self {
            StopReason::Entry => "entry",
            StopReason::Breakpoint => "breakpoint",
            StopReason::Step => "step",
            StopReason::Pause => "pause",
            StopReason::Exception => "exception",
        }
    }
}

/// Event sent to the host when execution stops
#[derive(Debug)]
pub struct DebugBreakEvent {
    pub reason: StopReason,
    pub location: Location,
    pub stack_frames: Vec<StackFrame>,
}

impl DebugBreakEvent {
    fn to_json(&self) -> String {
        let call_stack_json: Vec<String> = self.stack_frames.iter().map(|f| f.to_json()).collect();
        format!(
            r#"{{"reason":"{}","location":{},"stack_frames":[{}]}}"#,
            self.reason.as_str(),
            self.location.to_json(),
            call_stack_json.join(",")
        )
    }
}

/// Current execution location
#[derive(Debug, Clone)]
pub struct Location {
    pub filename: String,
    pub function: String,
    pub line: i32,
    pub column: i32,
}

impl Location {
    fn to_json(&self) -> String {
        format!(
            r#"{{"filename":"{}","function":"{}","line":{},"column":{}}}"#,
            escape_json_string(&self.filename),
            escape_json_string(&self.function),
            self.line,
            self.column
        )
    }
}

/// Action type returned from host
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugActionType {
    Continue,
    StepOver,
    StepInto,
    StepOut,
    Pause,
    Disconnect,
}

/// Action returned from the host debugger
#[derive(Debug)]
pub struct DebugAction {
    pub action: DebugActionType,
    pub breakpoints: Option<Vec<Breakpoint>>,
}

/// Global debugger state
pub struct DebuggerState {
    /// Whether the debugger is enabled
    pub enabled: bool,
    /// Current execution mode
    pub mode: DebugMode,
    /// Set of active breakpoints
    pub breakpoints: HashSet<Breakpoint>,
    /// Current call stack
    pub call_stack: Vec<StackFrame>,
    /// Stack depth when step over/out was initiated
    pub step_depth: usize,
    /// Whether this is the first operation (for entry breakpoint)
    pub first_op: bool,
    /// Stop at entry point
    pub stop_on_entry: bool,
    /// Last seen line number (to avoid duplicate breaks on same line)
    pub last_line: Option<i32>,
    /// Last seen filename
    pub last_file: Option<String>,
    /// Current function name being executed
    pub current_function: Option<String>,
}

impl Default for DebuggerState {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: DebugMode::Running,
            breakpoints: HashSet::new(),
            call_stack: Vec::new(),
            step_depth: 0,
            first_op: true,
            stop_on_entry: false,
            last_line: None,
            last_file: None,
            current_function: None,
        }
    }
}

impl DebuggerState {
    /// Check if we should break at the current location
    fn should_break(&self, filename: &str, line: i32, funcname: &str) -> Option<StopReason> {
        // Check for entry breakpoint
        if self.first_op && self.stop_on_entry {
            return Some(StopReason::Entry);
        }

        // Determine if we've moved to a different location
        let line_changed =
            self.last_line != Some(line) || self.last_file.as_deref() != Some(filename);
        let function_changed = self.current_function.as_deref() != Some(funcname);

        // Check execution mode
        match self.mode {
            DebugMode::Running => {
                // Only break at breakpoints, but avoid duplicate breaks on same line
                if line_changed && self.has_breakpoint(filename, line) {
                    Some(StopReason::Breakpoint)
                } else {
                    None
                }
            }
            DebugMode::Step => {
                // StepInto: Stop at the next line (different line or entering a new function)
                // This will enter into function calls
                if line_changed || function_changed {
                    Some(StopReason::Step)
                } else {
                    None
                }
            }
            DebugMode::StepOver => {
                // StepOver: Stop at next line in same or parent frame (skip over function calls)
                // Only stop if we're at the same or shallower stack depth AND line changed
                if self.call_stack.len() <= self.step_depth {
                    if line_changed {
                        Some(StopReason::Step)
                    } else {
                        None
                    }
                } else if self.has_breakpoint(filename, line) {
                    // Still respect breakpoints inside called functions
                    Some(StopReason::Breakpoint)
                } else {
                    None
                }
            }
            DebugMode::StepOut => {
                // StepOut: Stop at the first line after returning from current function
                // This means stack must be shallower than when we started
                if self.call_stack.len() < self.step_depth {
                    Some(StopReason::Step)
                } else if self.has_breakpoint(filename, line) {
                    // Still respect breakpoints while in the function
                    Some(StopReason::Breakpoint)
                } else {
                    None
                }
            }
            DebugMode::Paused => None,
        }
    }

    /// Check if there's a breakpoint at the given location
    fn has_breakpoint(&self, filename: &str, line: i32) -> bool {
        // Normalize the filename for comparison
        let normalized = normalize_path(filename);
        self.breakpoints.iter().any(|bp| {
            let bp_normalized = normalize_path(&bp.filename);
            bp_normalized == normalized && bp.line == line
        })
    }

    /// Update the call stack based on function name changes
    ///
    /// This tracks the call stack by monitoring when the `funcname` parameter changes.
    /// When the function name changes:
    /// - If the new function matches the function before the current one in the stack,
    ///   it's a return (step out) - pop the current frame
    /// - Otherwise, it's a call (step into) - push a new frame
    fn update_call_stack(&mut self, filename: &str, funcname: &str, line: i32, col: i32) {
        // Check if function has changed
        let function_changed = self.current_function.as_deref() != Some(funcname);

        if !function_changed {
            // Same function - just update location of the top frame if it exists
            if let Some(frame) = self.call_stack.last_mut() {
                frame.location.line = line;
                frame.location.column = col;
                frame.location.filename = filename.to_string();
            }
            return;
        }

        // Function has changed - determine if it's a call or return
        let stack_len = self.call_stack.len();

        // Check if we're returning to a parent function
        // Look at the function before the current top of stack
        let is_return = if stack_len >= 2 {
            // Get the parent frame (second from top)
            let parent_frame = &self.call_stack[stack_len - 2];
            parent_frame.name == funcname
        } else if stack_len == 1 {
            // If we only have one frame and function changed, check if we're returning
            // to the global scope or a different function
            false
        } else {
            false
        };

        if is_return {
            // Returning to parent function - pop the current frame
            self.call_stack.pop();
            // Update the parent frame's location
            if let Some(frame) = self.call_stack.last_mut() {
                frame.location.line = line;
                frame.location.column = col;
                frame.location.filename = filename.to_string();
            }
        } else {
            // Calling into a new function - push a new frame
            let new_frame = StackFrame {
                id: self.call_stack.len() as u32,
                name: funcname.to_string(),
                location: Location {
                    filename: filename.to_string(),
                    function: funcname.to_string(),
                    line,
                    column: col,
                },
                variables: Vec::new(),
            };
            self.call_stack.push(new_frame);
        }

        // Update current function tracking
        self.current_function = Some(funcname.to_string());
    }

    /// Handle a debug action from the host
    fn handle_action(&mut self, action: DebugAction) {
        // Update breakpoints
        if let Some(breakpoints) = action.breakpoints {
            self.breakpoints.clear();
            for bp in breakpoints {
                self.breakpoints.insert(bp);
            }
        }

        // Update execution mode
        match action.action {
            DebugActionType::Continue => {
                self.mode = DebugMode::Running;
            }
            DebugActionType::StepInto => {
                // Stop at the very next operation (go into function calls)
                self.mode = DebugMode::Step;
            }
            DebugActionType::StepOver => {
                // Stop at next operation in same or parent frame (skip over function calls)
                self.mode = DebugMode::StepOver;
                self.step_depth = self.call_stack.len();
            }
            DebugActionType::StepOut => {
                // Stop when returning to parent frame
                self.mode = DebugMode::StepOut;
                self.step_depth = self.call_stack.len();
            }
            DebugActionType::Pause => {
                // NOTE: Pause cannot be implemented because commands from the host
                // are only received when execution is already paused at a breakpoint.
                // There is no mechanism to interrupt execution mid-flight.
                // Treating as Continue for now.
                log::warn!(
                    "Pause action received but cannot be implemented - continuing execution"
                );
                self.mode = DebugMode::Running;
            }
            DebugActionType::Disconnect => {
                // Disable the debugger and continue execution to completion
                self.enabled = false;
                self.mode = DebugMode::Running;
            }
        }
    }
}

/// Escape a string for JSON output
fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                // Escape control characters as \uXXXX
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

/// Normalize a file path for comparison
fn normalize_path(path: &str) -> String {
    // Convert backslashes to forward slashes
    let path = path.replace('\\', "/");
    // Remove leading ./
    let path = path.strip_prefix("./").unwrap_or(&path);
    path.to_string()
}

/// Parse the debug action JSON response from host
fn parse_debug_action(json: &str) -> Option<DebugAction> {
    // Simple JSON parsing for the expected format:
    // {"action":"continue","breakpoints":[{"filename":"file.js","line":10}]}

    let action_type = if json.contains(r#""action":"continue""#) {
        DebugActionType::Continue
    } else if json.contains(r#""action":"StepInto""#) || json.contains(r#""action":"Step""#) {
        // "stepInto" or "step" -> stop at very next operation
        DebugActionType::StepInto
    } else if json.contains(r#""action":"StepOver""#) || json.contains(r#""action":"Next""#) {
        // "stepOver" or "next" -> stop at same/parent frame
        DebugActionType::StepOver
    } else if json.contains(r#""action":"StepOut""#) {
        DebugActionType::StepOut
    } else if json.contains(r#""action":"Pause""#) {
        DebugActionType::Pause
    } else if json.contains(r#""action":"Disconnect""#) {
        DebugActionType::Disconnect
    } else {
        // Default to continue if unknown
        DebugActionType::Continue
    };

    // Parse breakpoints array
    let mut breakpoints = None;

    // Find the breakpoints array
    if let Some(bp_start) = json.find(r#""breakpoints":["#) {
        let mut bps = Vec::new();
        let bp_section = &json[bp_start..];
        if let Some(array_end) = bp_section.find(']') {
            let array_content = &bp_section[15..array_end]; // Skip `"breakpoints":[`

            // Parse each breakpoint object
            for bp_str in array_content.split("},") {
                if let (Some(filename), Some(line)) = (
                    extract_json_string(bp_str, "filename"),
                    extract_json_number(bp_str, "line"),
                ) {
                    bps.push(Breakpoint { filename, line: line as i32 });
                }
            }
        }
        breakpoints = Some(bps);
    }

    Some(DebugAction {
        action: action_type,
        breakpoints,
    })
}

/// Extract a string value from a JSON object
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}":""#, key);
    if let Some(start) = json.find(&pattern) {
        let value_start = start + pattern.len();
        let remaining = &json[value_start..];
        // Find the closing quote (handling escaped quotes)
        let mut chars = remaining.chars().peekable();
        let mut result = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(result),
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        chars.next();
                        match next {
                            '"' => result.push('"'),
                            '\\' => result.push('\\'),
                            'n' => result.push('\n'),
                            'r' => result.push('\r'),
                            't' => result.push('\t'),
                            _ => {
                                result.push('\\');
                                result.push(next);
                            }
                        }
                    }
                }
                _ => result.push(c),
            }
        }
    }
    None
}

/// Extract a number value from a JSON object
fn extract_json_number(json: &str, key: &str) -> Option<u32> {
    let pattern = format!(r#""{}":"#, key);
    if let Some(start) = json.find(&pattern) {
        let value_start = start + pattern.len();
        let remaining = &json[value_start..];
        let num_str: String = remaining
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return num_str.parse().ok();
    }
    None
}

/// Get the global debugger state
fn debugger_state() -> &'static mut DebuggerState {
    #![allow(static_mut_refs)]
    static mut DEBUGGER_STATE: OnceCell<DebuggerState> = OnceCell::new();
    unsafe {
        DEBUGGER_STATE.get_or_init(DebuggerState::default);
        DEBUGGER_STATE.get_mut().unwrap()
    }
}

/// Enable the debugger
pub fn enable_debugger(stop_on_entry: bool) {
    let state = debugger_state();
    state.enabled = true;
    state.stop_on_entry = stop_on_entry;
    state.first_op = true;
}

/// Disable the debugger
pub fn disable_debugger() {
    let state = debugger_state();
    state.enabled = false;
}

/// Reset the debugger state for a new handler invocation
///
/// This should be called at the start of each new `register_handler` call or
/// when a registered handler is invoked. It clears the execution state while
/// preserving configuration settings (`enabled`, `stop_on_entry`, `breakpoints`).
///
/// This ensures that each handler invocation starts with a fresh state and
/// doesn't inherit stale call stack or stepping state from previous invocations.
pub fn reset_debugger_state() {
    let state = debugger_state();

    // Clear execution state (but preserve configuration)
    state.call_stack.clear();
    state.step_depth = 0;
    state.first_op = true;
    state.mode = DebugMode::Running;
    state.last_line = None;
    state.last_file = None;
    state.current_function = None;

    // Note: We intentionally preserve:
    // - enabled: Whether debugging is active
    // - stop_on_entry: Whether to break at entry point
    // - breakpoints: User-defined breakpoints
}

/// Check if the debugger is enabled
pub fn is_debugger_enabled() -> bool {
    debugger_state().enabled
}

/// Add a breakpoint
pub fn add_breakpoint(filename: String, line: i32) {
    debugger_state()
        .breakpoints
        .insert(Breakpoint { filename, line });
}

/// Remove a breakpoint
pub fn remove_breakpoint(filename: &str, line: i32) {
    let state = debugger_state();
    state
        .breakpoints
        .retain(|bp| !(bp.filename == filename && bp.line == line));
}

/// Called by the JS runtime on each operation
///
/// This is the main entry point for the debugger. It's called by the
/// `set_operation_changed` callback in main.rs.
///
/// Returns:
/// - 0 to continue execution normally
/// - Non-zero to interrupt execution (not yet implemented)
pub fn debug_trace_handler(
    ctx: &Ctx,
    filename: &str,
    funcname: &str,
    line: i32,
    col: i32,
) -> i32 {
    let state = debugger_state();

    if !state.enabled {
        return 0;
    }

    // Update call stack based on function name changes
    state.update_call_stack(filename, funcname, line, col);

    // Check if we should break (pass funcname for function change detection)
    let reason = state.should_break(filename, line, funcname);
    state.last_line = Some(line);
    state.last_file = Some(filename.to_string());

    let reason = match reason {
        Some(r) => r,
        None => return 0,
    };

    // Clear first_op flag after checking
    state.first_op = false;

    let depth = ctx.stack_depth();

    // for level in 0..depth {
    //     let variables = Vec::new();
    //     let vars = ctx.local_variables_at_level(level);
    //     vars.map(|vars| {
    //         for v in vars.iter() {
    //             v.name =
    //         }
    //     }
    //
    // }
    // Build the break event
    let mut stack_frames = state.call_stack.clone();
    stack_frames.reverse();

    let event = DebugBreakEvent {
        reason,
        location: Location {
            filename: filename.to_string(),
            function: funcname.to_string(),
            line,
            column: col,
        },
        stack_frames,
    };

    // Serialize and send to host
    let event_json = event.to_json();

    // Call host function and get response
    let response = match call_host_function::<String>(
        DEBUG_BREAK_FUNC_NAME,
        Some(vec![ParameterValue::String(event_json)]),
        ReturnType::String,
    ) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to call debug break host function: {:?}", e);
            return 0;
        }
    };

    // Parse the action from host
    let action = match parse_debug_action(&response) {
        Some(a) => a,
        None => {
            log::error!("Failed to parse debug action from: {}", response);
            // Default to continue on parse error
            DebugAction {
                action: DebugActionType::Continue,
                breakpoints: None,
            }
        }
    };

    // Handle the action
    state.handle_action(action);

    0 // Continue execution
}
