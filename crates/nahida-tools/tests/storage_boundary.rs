//! T02 diagnostic probes, not a production auth helper or sandbox fix.
//! Run in Linux CI via `make check`; never confine Cargo's shared test process.

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("storage boundary probes: Linux-only; not exercised on this platform");
}

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{self, OpenOptions};
    use std::io::{ErrorKind, Read as _, Write as _};
    use std::os::fd::AsRawFd as _;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const CASES: &[&str] = &[
        "outside_store_denied",
        "runtime_before_confinement",
        "runtime_after_confinement",
        "preopened_session",
        "private_pipe_reachable",
        "guarded_pipe_private",
        "guarded_ptrace_private",
    ];

    pub fn run() {
        let args: Vec<_> = std::env::args_os().collect();
        if args.get(1).is_some_and(|arg| arg == "--inert-helper") {
            let parent = args[3].to_str().expect("parent pid").parse().expect("parent pid");
            inert_helper(args.get(2).is_some_and(|arg| arg == "guarded"), parent);
            return;
        }
        if args.get(1).is_some_and(|arg| arg == "--probe") {
            let case = args[2].to_str().expect("probe name");
            let base = Path::new(&args[3]);
            match case {
                "outside_store_denied" => outside_store_denied(base),
                "runtime_before_confinement" => runtime_before_confinement(base),
                "runtime_after_confinement" => runtime_after_confinement(base),
                "preopened_session" => preopened_session(base),
                "private_pipe_reachable" => private_pipe_reachable(base),
                "guarded_pipe_private" => guarded_pipe_private(base),
                "guarded_ptrace_private" => guarded_ptrace_private(base),
                _ => panic!("unknown probe"),
            }
            return;
        }

        // The unrestricted parent owns cleanup, including files outside each
        // child's workspace. No child owns a TempDir it can no longer remove.
        for case in CASES {
            let base = tempfile::tempdir().expect("probe directory");
            fs::create_dir(base.path().join("workspace")).expect("workspace");
            fs::create_dir(base.path().join("store")).expect("fake store");
            let mut child = Command::new(std::env::current_exe().expect("probe executable"))
                .arg("--probe")
                .arg(case)
                .arg(base.path())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("probe child");
            let deadline = Instant::now() + Duration::from_secs(15);
            while child.try_wait().expect("probe status").is_none() {
                if Instant::now() >= deadline {
                    child.kill().expect("terminate stalled probe");
                    child.wait().expect("reap stalled probe");
                    panic!("{case}: timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().expect("probe output");
            assert!(
                output.status.success(),
                "{case}: {}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            println!("storage boundary probe {case}: passed");
        }
    }

    fn confine(base: &Path) {
        assert!(
            nahida_tools::confine_writes(&base.join("workspace"), |msg| eprintln!("{msg}"))
                .expect("Landlock"),
            "Linux probe requires fully enforced Landlock; unavailability is not a pass"
        );
    }

    fn denied(result: std::io::Result<()>) {
        assert_eq!(
            result.expect_err("outside write must fail").kind(),
            ErrorKind::PermissionDenied
        );
    }

    fn shell_write(path: &Path) -> bool {
        Command::new("bash")
            .args(["-c", "printf fake > \"$1\"", "probe"])
            .arg(path)
            .status()
            .expect("bash probe")
            .success()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn outside_store_denied(base: &Path) {
        let record = base.join("store/record");
        fs::write(&record, b"fake-generation-0").expect("seed fake record");
        confine(base);
        fs::write(base.join("workspace/ok"), b"fake").expect("workspace write");
        denied(fs::create_dir(base.join("store/sessions")));
        denied(fs::write(base.join("store/rotation.tmp"), b"fake-generation-1"));
        denied(fs::rename(&record, base.join("store/replaced")));
        assert_eq!(fs::read(&record).expect("old record"), b"fake-generation-0");
        assert!(!shell_write(&record));
    }

    fn runtime_before_confinement(base: &Path) {
        let runtime = runtime();
        // Force a worker to exist before applying the rules, like the default
        // #[tokio::main] startup. This is an expected counterexample, NOT a
        // desired production guarantee or proof the current CLI exploits it.
        runtime.block_on(async { tokio::spawn(async {}).await.expect("warm worker") });
        confine(base);
        denied(fs::write(base.join("store/main"), b"fake"));
        let outside = base.join("store/worker");
        runtime.block_on(async move {
            tokio::spawn(async move {
                assert!(shell_write(&outside), "preexisting worker's child remains unrestricted");
            })
            .await
            .expect("worker");
        });
        assert_eq!(fs::read(base.join("store/worker")).expect("counterexample"), b"fake");
    }

    fn runtime_after_confinement(base: &Path) {
        confine(base);
        let outside = base.join("store/worker");
        runtime().block_on(async move {
            tokio::spawn(async move {
                denied(fs::write(&outside, b"fake"));
                denied(tokio::fs::write(&outside, b"fake").await);
                assert!(!shell_write(&outside));
            })
            .await
            .expect("confined worker");
        });
        let outside = base.join("store/thread");
        std::thread::spawn(move || denied(fs::write(outside, b"fake")))
            .join()
            .expect("confined new thread");
    }

    fn preopened_session(base: &Path) {
        let path = base.join("store/session.jsonl");
        let mut file =
            OpenOptions::new().create_new(true).append(true).open(&path).expect("session");
        let fd = file.as_raw_fd();
        confine(base);
        writeln!(file, "fake-settled-entry").expect("preopened append capability");
        file.flush().expect("flush");
        assert!(!shell_write(&path));
        assert!(!shell_write(Path::new(&format!("/proc/{}/fd/{fd}", std::process::id()))));
        let closed = Command::new("bash")
            .args(["-c", "test ! /proc/self/fd/\"$1\" -ef \"$2\"", "probe"])
            .arg(fd.to_string())
            .arg(&path)
            .status()
            .expect("close-on-exec probe");
        assert!(closed.success(), "the session handle must not survive exec");
        assert_eq!(fs::read(path).expect("session contents"), b"fake-settled-entry\n");
    }

    // These guards apply only to disposable probe processes. No production
    // helper, credential isolation or new supported topology follows from them.
    fn guard_process() {
        let zero: libc::c_long = 0;
        // SAFETY: prctl scalar operations use no pointer arguments. Varargs
        // match the API's long argument width, including on 32-bit Linux.
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, zero, zero, zero, zero) }, 0);
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, zero, zero, zero, zero) }, 0);
    }

    fn inert_helper(guarded: bool, parent: libc::pid_t) {
        // SAFETY: only scalar arguments. Kill this test-owned helper if its
        // probe parent is terminated by the outer watchdog, including stop states.
        let signal: libc::c_long = libc::SIGKILL.into();
        let zero: libc::c_long = 0;
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, signal, zero, zero, zero) }, 0);
        // A dead parent might have been replaced by a subreaper, not just PID 1.
        assert_eq!(unsafe { libc::getppid() }, parent, "probe parent already exited");
        if guarded {
            guard_process();
        }
        std::io::stdout().write_all(b"R").expect("ready");
        std::io::stdout().flush().expect("ready flush");
        // A bounded inert echo, NOT a storage protocol. Empty EOF models cancel;
        // one fixed ping models trusted traffic. Never accepts paths or commands.
        let mut bytes = Vec::new();
        std::io::stdin().take(5).read_to_end(&mut bytes).expect("inert input");
        match bytes.as_slice() {
            b"" => (),
            b"ping" => std::io::stdout().write_all(b"pong").expect("echo"),
            _ => panic!("invalid inert input"),
        }
    }

    fn helper(guarded: bool) -> std::process::Child {
        let mut child = Command::new(std::env::current_exe().expect("probe executable"))
            .arg("--inert-helper")
            .arg(if guarded { "guarded" } else { "dumpable" })
            .arg(std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("inert helper");
        let mut ready = [0];
        child.stdout.as_mut().expect("stdout").read_exact(&mut ready).expect("ready");
        assert_eq!(ready, *b"R");
        child
    }

    fn guarded_pipe_private(base: &Path) {
        guard_process();
        let mut child = helper(true);
        let control = child.stdin.as_ref().expect("control").as_raw_fd();
        confine(base);
        // Same-domain parent descriptor, like the unguarded positive control.
        // Read/write procfs access must fail; CLOEXEC alone did not suffice.
        let parent_fd = format!("/proc/{}/fd/{control}", std::process::id());
        assert!(!shell_write(Path::new(&parent_fd)));
        let denied_read = Command::new("bash")
            .args(["-c", "test ! -r \"$1\" && test ! -r \"$2\"", "probe"])
            .arg(&parent_fd)
            .arg(format!("/proc/{}/mem", child.id()))
            .status()
            .expect("procfs read probe");
        assert!(denied_read.success(), "control/memory must not be shell-readable");
        let closed = Command::new("bash")
            .args(["-c", "test ! -e /proc/self/fd/\"$1\"", "probe"])
            .arg(control.to_string())
            .status()
            .expect("exec descriptor probe");
        assert!(closed.success(), "control must not survive exec");
        assert!(!shell_write(&base.join("store/outside")));
        child.stdin.as_mut().expect("control").write_all(b"ping").expect("trusted ping");
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("reap helper after EOF");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"pong", "only trusted bytes reached helper");
        // Cancellation/empty EOF also terminates an owned helper. Its own guard
        // is established after exec, which otherwise resets dumpability.
        let mut cancelled = helper(true);
        drop(cancelled.stdin.take());
        let output = cancelled.wait_with_output().expect("reap cancelled helper");
        assert!(output.status.success());
        assert_eq!(output.stdout, Vec::<u8>::new(), "cancelled helper returned unexpected bytes");
    }

    fn guarded_ptrace_private(base: &Path) {
        confine(base);
        // Both targets inherit the caller's Landlock domain. The caller is the
        // target's parent: Yama's usual restricted-parent rule cannot make the
        // negative case pass without its dumpable positive control succeeding.
        for guarded in [false, true] {
            let mut child = helper(guarded);
            let pid = i32::try_from(child.id()).expect("pid");
            // SAFETY: PTRACE_ATTACH/DETACH take no data pointers. Only the exact
            // owned child is targeted, and the outer watchdog bounds all waits.
            let attached = unsafe {
                libc::ptrace(
                    libc::PTRACE_ATTACH,
                    pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    std::ptr::null_mut::<libc::c_void>(),
                )
            };
            let attach_error = std::io::Error::last_os_error();
            if guarded {
                assert_eq!(attached, -1, "non-dumpable target must refuse attach");
                assert_eq!(attach_error.raw_os_error(), Some(libc::EPERM));
            } else {
                assert_eq!(attached, 0, "positive control must attach; Yama denial is not proof");
                let mut status = 0;
                // SAFETY: status is a valid out-pointer; pid is our stopped child.
                assert_eq!(unsafe { libc::waitpid(pid, &raw mut status, libc::WUNTRACED) }, pid);
                assert!(libc::WIFSTOPPED(status));
                assert_eq!(
                    unsafe {
                        libc::ptrace(
                            libc::PTRACE_DETACH,
                            pid,
                            std::ptr::null_mut::<libc::c_void>(),
                            std::ptr::null_mut::<libc::c_void>(),
                        )
                    },
                    0
                );
            }
            drop(child.stdin.take());
            assert!(child.wait().expect("reap ptrace target").success());
        }
    }

    fn private_pipe_reachable(base: &Path) {
        // An inert echo child stands in for a pre-confinement helper. Even
        // close-on-exec alone need not protect the parent's control pipe:
        // procfs can reopen it when parent and shell share a Landlock domain.
        // No credential, storage RPC or privileged operation is implemented.
        let mut echo = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("inert echo child");
        let control = echo.stdin.as_ref().expect("control pipe").as_raw_fd();
        confine(base);
        let reachable =
            shell_write(Path::new(&format!("/proc/{}/fd/{control}", std::process::id())));
        drop(echo.stdin.take());
        let output = echo.wait_with_output().expect("reap echo child");
        assert!(output.status.success());
        assert!(reachable, "probe expects the unguarded parent pipe to be reachable");
        assert_eq!(output.stdout, b"fake", "shell injected bytes into the supposedly private pipe");
    }
}
