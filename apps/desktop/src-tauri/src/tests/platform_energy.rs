use super::*;

fn process(pid: u32, parent: Option<u32>, webview: bool) -> ProcessSample {
    ProcessSample {
        pid: Pid::from_u32(pid),
        parent: parent.map(Pid::from_u32),
        started: 1,
        webview,
        memory: 0,
        usage: Usage::default(),
    }
}

#[test]
fn separates_webviews_and_workloads_without_collecting_unrelated_processes() {
    // Reverse order exercises descendants found before their parents.
    let processes = [
        process(6, Some(5), true),
        process(5, Some(1), false),
        process(4, Some(3), false),
        process(3, Some(1), true),
        process(2, None, true),
        process(1, None, false),
    ];
    let roles = roles(&processes, Pid::from_u32(1));
    assert_eq!(roles.get(&Pid::from_u32(1)), Some(&Role::Native));
    assert_eq!(roles.get(&Pid::from_u32(3)), Some(&Role::Webview));
    assert_eq!(roles.get(&Pid::from_u32(4)), Some(&Role::Webview));
    assert_eq!(roles.get(&Pid::from_u32(5)), Some(&Role::Workload));
    assert_eq!(roles.get(&Pid::from_u32(6)), Some(&Role::Workload));
    assert!(!roles.contains_key(&Pid::from_u32(2)));
}

#[test]
fn counters_do_not_wrap_on_reset() {
    let previous = Usage {
        cpu_ms: 100,
        read: 300,
        written: 400,
    };
    let current = Usage {
        cpu_ms: 125,
        read: 200,
        written: 450,
    };
    let delta = delta(current, previous);
    assert_eq!(delta.cpu_ms, 25);
    assert_eq!(delta.read, 0);
    assert_eq!(delta.written, 50);
}

#[test]
fn helper_names_are_bounded() {
    assert!(is_webview("WebKitWebProcess"));
    assert!(is_webview("com.apple.WebKit.GPU"));
    assert!(is_webview("msedgewebview2.exe"));
    assert!(!is_webview("agent"));
}

#[test]
fn sampler_tracks_itself_and_discards_a_resume_gap() {
    let mut sampler = Sampler::new().expect("sysinfo supports this platform");
    sampler.sample(Context::Hidden);
    let pid = Pid::from_u32(std::process::id());
    assert!(sampler.previous.keys().any(|(tracked, _)| *tracked == pid));
    assert!(
        sampler
            .previous
            .keys()
            .all(|(tracked, _)| sampler.system.process(*tracked).is_some())
    );
    sampler.sampled_at = SystemTime::now().checked_sub(PERIOD * 3);
    sampler.sample(Context::Hidden);
    assert!(
        sampler
            .sampled_at
            .is_some_and(|at| at.elapsed().is_ok_and(|elapsed| elapsed < PERIOD))
    );
}

/// Run explicitly on the target laptop to measure collector cost without waiting minutes.
#[test]
#[ignore]
fn sampling_cost() {
    let mut sampler = Sampler::new().expect("sysinfo supports this platform");
    let mut durations = Vec::new();

    for _ in 0..60 {
        let started = Instant::now();
        sampler.sample(Context::Hidden);
        durations.push(started.elapsed());
    }

    durations.sort();
    println!(
        "collector median: {:?}, p95: {:?}",
        durations[30], durations[57]
    );
}

#[test]
fn transitions_and_clock_gaps_start_a_fresh_baseline() {
    let now = SystemTime::now();
    let sampler = Sampler {
        sampled_at: now.checked_sub(PERIOD),
        previous_context: Some((Context::Hidden, PowerSource::Battery)),
        ..Sampler::new().expect("sysinfo supports this platform")
    };
    assert!(sampler.continuous(now, Context::Hidden, PowerSource::Battery));
    assert!(!sampler.continuous(now, Context::Visible, PowerSource::Battery));
    assert!(!sampler.continuous(now, Context::Hidden, PowerSource::External));
    assert!(!sampler.continuous(now + PERIOD * 3, Context::Hidden, PowerSource::Battery));
    assert!(!sampler.continuous(now - PERIOD * 3, Context::Hidden, PowerSource::Battery));
    assert!(
        !Sampler::new()
            .expect("sysinfo supports this platform")
            .continuous(now, Context::Hidden, PowerSource::Battery)
    );
}
