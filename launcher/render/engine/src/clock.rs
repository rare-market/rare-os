//! CLOCK_MONOTONIC in nanoseconds: the same clock the journeys harness stamps its inputs and
//! captured frames with, so engine logs line up with the capture.

use std::sync::OnceLock;

pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes into the timespec we pass.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub fn realtime_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: as above.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

static START: OnceLock<u64> = OnceLock::new();

/// Call first thing in main so `since_start` measures from process start.
pub fn mark_start() -> u64 {
    *START.get_or_init(mono_ns)
}

/// Seconds since `mark_start`, the time base of every animation.
pub fn secs() -> f64 {
    let s = *START.get_or_init(mono_ns);
    (mono_ns().saturating_sub(s)) as f64 / 1e9
}

pub fn ns_to_secs(ns: u64) -> f64 {
    let s = *START.get_or_init(mono_ns);
    (ns.saturating_sub(s)) as f64 / 1e9
}
