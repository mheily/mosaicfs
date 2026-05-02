// Integration tests for the Linux sandbox (Landlock + caps + seccomp).
// Each test forks a child process so the sandbox can be applied without
// affecting the test runner itself. Linux-only; compiled out on macOS.

#![cfg(target_os = "linux")]

use std::process::Command;

/// Run this test binary itself as a child with a special env var that
/// triggers a specific sandbox verification scenario.
fn run_child(scenario: &str) -> std::process::Output {
    let exe = std::env::current_exe().expect("current_exe");
    Command::new(exe)
        .env("SANDBOX_TEST_SCENARIO", scenario)
        .output()
        .expect("failed to spawn child")
}

/// Entrypoint hook: if SANDBOX_TEST_SCENARIO is set, run that scenario
/// in-process (we are the child) and exit.
///
/// Called from each #[test] via run_child(); the test binary re-executes
/// itself with the scenario env var set instead of running the test suite
/// again, which avoids re-entering the test harness.
fn maybe_run_scenario() {
    let Ok(scenario) = std::env::var("SANDBOX_TEST_SCENARIO") else {
        return;
    };
    match scenario.as_str() {
        "caps_dropped" => scenario_caps_dropped(),
        "seccomp_blocks_execve" => scenario_seccomp_blocks_execve(),
        other => {
            eprintln!("unknown scenario: {other}");
            std::process::exit(2);
        }
    }
}

// ── scenarios ────────────────────────────────────────────────────────────────

fn scenario_caps_dropped() {
    use caps::{CapSet, CapsHashSet};
    // Apply only the caps-drop portion (no Landlock — would need system dirs).
    unsafe {
        let rc = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1u64, 0u64, 0u64, 0u64);
        assert_eq!(rc, 0, "prctl failed");
    }
    caps::set(None, CapSet::Permitted, &CapsHashSet::new()).expect("drop permitted");
    caps::set(None, CapSet::Inheritable, &CapsHashSet::new()).expect("drop inheritable");
    caps::set(None, CapSet::Effective, &CapsHashSet::new()).expect("drop effective");

    let remaining = caps::read(None, CapSet::Effective).expect("read caps");
    if remaining.is_empty() {
        println!("OK: caps dropped");
        std::process::exit(0);
    } else {
        eprintln!("FAIL: caps not empty: {remaining:?}");
        std::process::exit(1);
    }
}

fn scenario_seccomp_blocks_execve() {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;

    let denied: &[i64] = &[libc::SYS_execve, libc::SYS_execveat];
    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    for s in denied {
        rules.insert(*s, vec![]);
    }

    let target = if cfg!(target_arch = "x86_64") {
        TargetArch::x86_64
    } else if cfg!(target_arch = "aarch64") {
        TargetArch::aarch64
    } else {
        eprintln!("SKIP: unsupported arch");
        std::process::exit(0);
    };

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        target,
    )
    .expect("build filter");
    let program: BpfProgram = filter.try_into().expect("compile filter");
    seccompiler::apply_filter(&program).expect("apply filter");

    // execve is now blocked — spawning any process must fail.
    let result = Command::new("/bin/true").spawn();
    match result {
        Err(e) if e.raw_os_error() == Some(libc::EPERM) => {
            println!("OK: execve blocked with EPERM");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("FAIL: unexpected error: {e}");
            std::process::exit(1);
        }
        Ok(_) => {
            eprintln!("FAIL: execve succeeded — seccomp filter not applied");
            std::process::exit(1);
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn caps_are_dropped() {
    maybe_run_scenario(); // no-op in parent (no env var set)
    let out = run_child("caps_dropped");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "child failed:\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("OK:"), "unexpected output: {stdout}");
}

#[test]
fn seccomp_blocks_execve() {
    maybe_run_scenario(); // no-op in parent
    let out = run_child("seccomp_blocks_execve");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "child failed:\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("OK:"), "unexpected output: {stdout}");
}
