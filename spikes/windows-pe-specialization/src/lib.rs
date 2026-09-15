#[cfg(not(windows))]
compile_error!("the Windows PE specialization spike must be built and run on Windows");

pub mod pe;
