#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssMeasurement {
    pub peak_rss_bytes: Option<usize>,
    pub unavailable_reason: Option<String>,
}

#[must_use]
pub fn measure() -> RssMeasurement {
    platform_measure()
}

#[cfg(windows)]
fn platform_measure() -> RssMeasurement {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        cb: core::mem::size_of::<Counters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let ok = unsafe { GetProcessMemoryInfo((-1isize) as *mut _, &mut counters, counters.cb) } != 0;
    if ok && counters.peak_working_set_size > 0 {
        RssMeasurement {
            peak_rss_bytes: Some(counters.peak_working_set_size),
            unavailable_reason: None,
        }
    } else {
        RssMeasurement {
            peak_rss_bytes: None,
            unavailable_reason: Some("GetProcessMemoryInfo unavailable".into()),
        }
    }
}

#[cfg(unix)]
fn platform_measure() -> RssMeasurement {
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut RUsage) -> i32;
    }
    #[repr(C)]
    struct Time {
        sec: isize,
        usec: isize,
    }
    #[repr(C)]
    struct RUsage {
        user: Time,
        system: Time,
        maxrss: isize,
        rest: [isize; 14],
    }
    let mut usage = RUsage {
        user: Time { sec: 0, usec: 0 },
        system: Time { sec: 0, usec: 0 },
        maxrss: 0,
        rest: [0; 14],
    };
    if unsafe { getrusage(0, &mut usage) } == 0 && usage.maxrss > 0 {
        let multiplier = if cfg!(target_os = "macos") { 1 } else { 1024 };
        RssMeasurement {
            peak_rss_bytes: Some(usage.maxrss as usize * multiplier),
            unavailable_reason: None,
        }
    } else {
        RssMeasurement {
            peak_rss_bytes: None,
            unavailable_reason: Some("getrusage unavailable".into()),
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn platform_measure() -> RssMeasurement {
    RssMeasurement {
        peak_rss_bytes: None,
        unavailable_reason: Some("OS RSS API unavailable".into()),
    }
}
