//! OS-level filesystem confinement, Linux only (Landlock).
//!
//! [`Sandbox::resolve`](crate::Sandbox::resolve) only constrains paths the
//! model passes to `read`/`write` — `bash` runs with this process's full
//! filesystem access. This closes that specific gap the way grok-build's
//! `xai-grok-sandbox` does: a kernel-enforced restriction applied once at
//! process startup, covering every subprocess `bash` spawns from that point
//! on, with no API to lift it afterward.
//!
//! Read and execute access are left completely unrestricted — this only
//! denies write/create/delete/rename operations outside the workspace root.
//! That's a scoped decision, not a first step toward more: confining reads
//! too needs a comprehensive allowlist of whatever paths the host distro's
//! dynamic linker, DNS resolution, and TLS trust store need, which varies
//! enough between distros — notably Nix, where nearly everything lives under
//! content-addressed `/nix/store` paths, not a fixed `/usr` layout — that
//! getting it wrong would break `bash` outright. Reading a secret the model
//! was never supposed to see, or exfiltrating it over the network, is a
//! separate risk this does not address; permission gating
//! ([`Tool::requires_confirmation`](nahida_agent::Tool::requires_confirmation))
//! is the layer meant to catch a call before it runs at all.
//!
//! macOS has no equivalent here: `sandbox-exec`/Seatbelt is deprecated with
//! no documented replacement for headless process sandboxing, and a naive
//! Seatbelt profile is known to break `reqwest`'s default macOS proxy
//! detection (blocks the `com.apple.SystemConfiguration.configd` Mach
//! service) — which would break the network calls this agent needs to
//! function at all. Not implemented until that has a real answer.

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;

    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus,
    };

    /// Confine write/create/delete access to `workspace_root`; leave read and
    /// execute unrestricted everywhere. Applies to this process and every
    /// child it spawns from this point on — there is no API to lift it.
    ///
    /// Best-effort: an older kernel, Landlock built out of the running
    /// kernel, or Landlock disabled via LSM config all degrade to "not
    /// enforced" rather than a hard error — refusing to start over an
    /// unrelated kernel config choice would be the wrong failure mode.
    /// `log` is called once, only when that happens, so the caller can
    /// surface it without this module knowing what a terminal is.
    ///
    /// Returns whether confinement actually ended up fully enforced — `Ok`
    /// alone only means the Landlock calls themselves didn't hard-error, not
    /// that writes are actually confined (that's the degraded case `log`
    /// reports). A caller that needs to know "is this real" (`nahida
    /// --describe`, for one) has to check the bool, not just `.is_ok()`.
    pub fn confine_writes(
        workspace_root: &Path,
        log: impl FnOnce(&str),
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let abi = ABI::V5;

        let status = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(abi))?
            .create()?
            .add_rule(PathBeneath::new(PathFd::new("/")?, AccessFs::from_read(abi)))?
            .add_rule(PathBeneath::new(PathFd::new(workspace_root)?, AccessFs::from_all(abi)))?
            .restrict_self()?;

        let enforced = status.ruleset == RulesetStatus::FullyEnforced;
        if !enforced {
            log("landlock: not fully enforced on this kernel — bash runs without OS-level \
                 write confinement (permission gating still applies)");
        }

        Ok(enforced)
    }
}

#[cfg(target_os = "linux")]
pub use linux::confine_writes;

// `restrict_self()` is irreversible for the whole process, so this can never
// share a process with the rest of the suite — `cargo test` runs every test
// in one binary by default, and a write restriction applied by one test
// would silently break every tempfile-based test that runs after it. Kept
// `#[ignore]`d and run alone, deliberately, rather than folded into `make
// check`: `cargo test -p nahida-tools --lib -- --ignored --exact
// os_sandbox::linux_tests::confine_writes_actually_blocks_writes_outside_the_root`
#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::linux::confine_writes;

    #[test]
    #[ignore = "irreversible for the whole process — run alone, Linux only"]
    fn confine_writes_actually_blocks_writes_outside_the_root() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::NamedTempFile::new().expect("outside tempfile");

        let enforced =
            confine_writes(workspace.path(), |msg| eprintln!("{msg}")).expect("confine_writes");
        assert!(enforced, "this test's whole premise is that it actually got enforced");

        // Inside the confined root: still writable.
        std::fs::write(workspace.path().join("ok.txt"), b"hi")
            .expect("write inside the root must still succeed");

        // Outside it: the kernel refuses, not just this process's own logic
        // pretending to. This is the one assertion that actually proves
        // enforcement rather than just "the function returned Ok".
        let err = std::fs::OpenOptions::new()
            .write(true)
            .open(outside.path())
            .expect_err("write outside the root must be denied by the kernel");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "got {err:?}");
    }
}

/// No-op on every platform except Linux — see the module doc for why macOS
/// has no equivalent yet. Always reports `Ok(false)`: never enforced here.
#[cfg(not(target_os = "linux"))]
pub fn confine_writes(
    _workspace_root: &std::path::Path,
    log: impl FnOnce(&str),
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    log("OS-level write confinement is Linux-only (Landlock) — not applied on this platform. \
         Permission gating still applies to bash.");
    Ok(false)
}
