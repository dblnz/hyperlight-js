//! Thin QuickJS adapter over the debugger state machine in [`crate::guest::dap`].
//!
//! This module wires the QuickJS trace hook into the shared debugger state machine
//! and re-exports the public API that `hyperlight.rs` depends on.

use alloc::format;
use alloc::string::ToString;
use anyhow::Result;
use rquickjs::Context;

use crate::guest::dap;

pub struct Debugger;

impl crate::debugger::Debugger for Debugger {
    fn enable_debugging(&self, ctx: &Context) -> Result<()> {
        ctx.runtime().set_debug_trace_handler(
            ctx,
            Some(alloc::boxed::Box::new(
                |context, filename, funcname, line, col| {
                    dap::on_trace_event_with(filename, funcname, line, col, &|level| {
                        context
                            .local_variables_at_level(level as i32)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|v| dap::Variable {
                                name: v.name,
                                value: format!("{:?}", v.value),
                                type_name: Some(
                                    if v.is_arg {
                                        "argument"
                                    } else {
                                        "local"
                                    }
                                    .to_string(),
                                ),
                            })
                            .collect()
                    })
                },
            )),
        );
        Ok(())
    }
}

/// Enable the debugger with optional stop-on-entry.
pub fn enable_debugger(stop_on_entry: bool) {
    dap::enable(stop_on_entry);
}

/// Reset per-invocation debugger state (call at handler registration / dispatch).
pub fn reset_debugger_state() {
    dap::reset();
}
