//! Insertable local date/time, with platform-specific formatting.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::current;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::current;
