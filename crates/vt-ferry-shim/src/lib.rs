#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
// Apple framework symbols use lowercase-`k` prefixes
// (`kCVPixelBufferPixelFormatTypeKey` etc.) and the storage-side
// helpers we re-export them through inherit those names. Rust's
// SCREAMING_SNAKE_CASE convention can't apply here without making
// the FFI surface unrecognizable to ffmpeg / VideoToolbox callers.
#![allow(non_upper_case_globals)]

pub mod corefoundation;
pub mod coremedia;
pub mod corevideo;
pub mod runtime;
pub mod transport;
pub mod videotoolbox;
