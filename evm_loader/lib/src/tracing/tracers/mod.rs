use crate::tracing::tracers::struct_logger::StructLogger;
use crate::tracing::TraceConfig;
use evm_loader::evm::tracing::TracerType;
use std::cell::RefCell;
use std::rc::Rc;

pub mod struct_logger;

pub fn new_tracer(trace_config: &TraceConfig) -> evm_loader::error::Result<TracerType> {
    Ok(Rc::new(RefCell::new(
        match trace_config.tracer.as_deref() {
            None | Some("") => Box::new(StructLogger::new(trace_config)),
            _ => {
                return Err(evm_loader::error::Error::Custom(format!(
                    "Unsupported tracer: {:?}",
                    trace_config.tracer
                )))
            }
        },
    )))
}
