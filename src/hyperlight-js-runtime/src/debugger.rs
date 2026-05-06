use anyhow::Result;
use rquickjs::Context;

/// A trait representing the debugger for the JS runtime. This allows the host to enable
/// debugging for the JS runtime, which allows the runtime to report debugging information
pub trait Debugger {
    /// Enable debugging for the JS runtime. This will allow the runtime to report
    /// debugging information to the host, such as the current call stack and variable values.
    fn enable_debugging(&self, ctx: &Context) -> Result<()>;
}
