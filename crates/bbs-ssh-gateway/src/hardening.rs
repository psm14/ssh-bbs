use tracing::warn;

pub fn apply() {
    #[cfg(unix)]
    {
        set_umask();
        disable_core_dumps();
        set_no_new_privs();
    }
}

#[cfg(unix)]
fn set_umask() {
    unsafe {
        libc::umask(0o077);
    }
}

#[cfg(unix)]
fn disable_core_dumps() {
    let rlimit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    let res = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlimit) };
    if res != 0 {
        warn!(errno = res, "setting RLIMIT_CORE to 0 failed (continuing)");
    }
}

#[cfg(all(unix, target_os = "linux"))]
fn set_no_new_privs() {
    let res = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS as _, 1, 0, 0, 0) };
    if res != 0 {
        warn!(
            errno = res,
            "prctl(PR_SET_NO_NEW_PRIVS) failed (continuing)"
        );
    }
}

#[cfg(any(not(unix), all(unix, not(target_os = "linux"))))]
fn set_no_new_privs() {}
