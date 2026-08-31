#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssMeasurement {
    pub peak_rss_bytes: Option<usize>,
    pub unavailable_reason: Option<String>,
}

#[must_use]
pub fn measure() -> RssMeasurement {
    platform_measure()
}

#[must_use]
pub fn measure_process(pid: u32) -> RssMeasurement {
    process_measure(pid)
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
        #[link_name = "GetProcessMemoryInfo"]
        fn get_process_memory_info(
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
    let ok =
        unsafe { get_process_memory_info((-1isize) as *mut _, &mut counters, counters.cb) } != 0;
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

#[cfg(unix)]
fn process_measure(pid: u32) -> RssMeasurement {
    let path = format!("/proc/{pid}/status");
    let Ok(status) = std::fs::read_to_string(path) else {
        return RssMeasurement {
            peak_rss_bytes: None,
            unavailable_reason: Some("/proc status unavailable".into()),
        };
    };
    let value = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?.split_whitespace().next()?;
        value.parse::<usize>().ok().map(|kb| kb * 1024)
    });
    RssMeasurement {
        peak_rss_bytes: value,
        unavailable_reason: value.is_none().then(|| "VmRSS unavailable".into()),
    }
}

#[cfg(windows)]
fn process_measure(pid: u32) -> RssMeasurement {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output();
    let Ok(output) = output else {
        return RssMeasurement {
            peak_rss_bytes: None,
            unavailable_reason: Some("tasklist unavailable".into()),
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text.rsplit_once("\",\"").and_then(|(_, field)| {
        field
            .trim_matches(['"', ' ', '\r', '\n'])
            .replace(',', "")
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .map(|kb| kb * 1024)
    });
    let value = value.or_else(|| {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).WorkingSet64"),
            ])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| text.trim().parse::<usize>().ok())
    });
    RssMeasurement {
        peak_rss_bytes: value,
        unavailable_reason: value.is_none().then(|| "tasklist RSS unavailable".into()),
    }
}

#[cfg(not(any(unix, windows)))]
fn process_measure(_pid: u32) -> RssMeasurement {
    RssMeasurement {
        peak_rss_bytes: None,
        unavailable_reason: Some("OS RSS API unavailable".into()),
    }
}

#[cfg(test)]
#[test]
fn process_measurement_is_callable_for_current_process() {
    let _ = measure_process(std::process::id());
}
