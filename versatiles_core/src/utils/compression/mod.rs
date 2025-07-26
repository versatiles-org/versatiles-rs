mod compression_goal;
mod functions;
mod method_brotli;
mod method_gzip;
mod target_compression;
#[cfg(test)]
pub mod tests;

pub use functions::*;
pub use method_brotli::*;
pub use method_gzip::*;
pub use target_compression::*;
