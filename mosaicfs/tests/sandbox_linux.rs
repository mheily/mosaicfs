// Integration tests for the Linux sandbox (caps drop + seccomp).
//
// Uses harness=false so we control main() on both sides of the fork:
//   - parent: discovers and runs test functions, reports results
//   - child:  SANDBOX_TEST_SCENARIO env var is set; runs the named scenario
//             and exits with code 0 (pass) or 1 (fail)
//
// Landlock is not tested here because it requires the system dirs
// (/var/lib/mosaicfs etc.) that only exist in a real deployment.

use std::process::Command;

// ── scenario runners (child side) ────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn scenario_caps_dropped() {
    use caps::{CapSet, CapsHashSet};
    unsafe {
        let rc = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1u64, 0u64, 0u64, 0u64);
        if rc != 0 {
            eprintln!("FAIL: prctl: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
    }
    if let Err(e) = caps::set(None, CapSet::Permitted, &CapsHashSet::new()) {
        eprintln!("FAIL: drop permitted: {e}");
        std::process::exit(1);
    }
    if let Err(e) = caps::set(None, CapSet::Inheritable, &CapsHashSet::new()) {
        eprintln!("FAIL: drop inheritable: {e}");
        std::process::exit(1);
    }
    if let Err(e) = caps::set(None, CapSet::Effective, &CapsHashSet::new()) {
        eprintln!("FAIL: drop effective: {e}");
        std::process::exit(1);
    }
    match caps::read(None, CapSet::Effective) {
        Ok(remaining) if remaining.is_empty() => {
            eprintln!("OK: caps dropped");
            std::process::exit(0);
        }
        Ok(remaining) => {
            eprintln!("FAIL: caps not empty: {remaining:?}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("FAIL: read caps: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn scenario_seccomp_blocks_execve() {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;

    let target = if cfg!(target_arch = "x86_64") {
        TargetArch::x86_64
    } else if cfg!(target_arch = "aarch64") {
        TargetArch::aarch64
    } else {
        eprintln!("SKIP: unsupported arch");
        std::process::exit(0);
    };

    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    rules.insert(libc::SYS_execve, vec![]);
    rules.insert(libc::SYS_execveat, vec![]);

    let filter = match SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        target,
    ) {
        Ok(f) => f,
        Err(e) => { eprintln!("FAIL: build filter: {e}"); std::process::exit(1); }
    };
    let program: BpfProgram = match filter.try_into() {
        Ok(p) => p,
        Err(e) => { eprintln!("FAIL: compile filter: {e}"); std::process::exit(1); }
    };
    if let Err(e) = seccompiler::apply_filter(&program) {
        eprintln!("FAIL: apply filter: {e}");
        std::process::exit(1);
    }

    match Command::new("/bin/true").spawn() {
        Err(e) if e.raw_os_error() == Some(libc::EPERM) => {
            eprintln!("OK: execve blocked with EPERM");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("FAIL: unexpected error: {e}");
            std::process::exit(1);
        }
        Ok(_) => {
            eprintln!("FAIL: execve succeeded — filter not applied");
            std::process::exit(1);
        }
    }
}

// ── test functions (parent side) ─────────────────────────────────────────────

fn run_child(scenario: &str) -> std::process::Output {
    let exe = std::env::current_exe().expect("current_exe");
    Command::new(exe)
        .env("SANDBOX_TEST_SCENARIO", scenario)
        .output()
        .expect("failed to spawn child")
}

fn test_caps_are_dropped() {
    let out = run_child("caps_dropped");
    assert!(
        out.status.success(),
        "child exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn test_seccomp_blocks_execve() {
    let out = run_child("seccomp_blocks_execve");
    assert!(
        out.status.success(),
        "child exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── main ─────────────────────────────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("1..0 # SKIP not Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    // Child path: run the named scenario and exit.
    if let Ok(scenario) = std::env::var("SANDBOX_TEST_SCENARIO") {
        match scenario.as_str() {
            "caps_dropped"           => scenario_caps_dropped(),
            "seccomp_blocks_execve"  => scenario_seccomp_blocks_execve(),
            other => {
                eprintln!("unknown scenario: {other}");
                std::process::exit(2);
            }
        }
        unreachable!();
    }

    // Parent path: run tests and report.
    type TestFn = fn();
    let tests: &[(&str, TestFn)] = &[
        ("caps_are_dropped",      test_caps_are_dropped),
        ("seccomp_blocks_execve", test_seccomp_blocks_execve),
    ];

    println!("running {} tests", tests.len());
    let mut failed = Vec::new();
    for (name, f) in tests {
        let result = std::panic::catch_unwind(f);
        match result {
            Ok(()) => println!("test {name} ... ok"),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
                    .or_else(|| e.downcast_ref::<&str>().copied())
                    .unwrap_or("(panic)");
                println!("test {name} ... FAILED\n  {msg}");
                failed.push(name);
            }
        }
    }

    let n_ok = tests.len() - failed.len();
    println!(
        "\ntest result: {}. {n_ok} passed; {} failed",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}
