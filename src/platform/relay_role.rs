//! The relay role: one process serves the bypass, the others wait.
//!
//! Two relays on one machine is not a redundancy, it is a fault. Both bind the
//! same addresses (the gate hosts' door on `:443`, the DNS relay on
//! `127.0.0.53:53`, the local proxy), and only one of them can win each of
//! them - so the loser answers the gate hosts with Google's real address while
//! the winner holds their door, and the two of them overwrite each other's
//! record of what is going on. Measured on a machine with both an installed and
//! a portable relay: the installed one held nothing at all, yet it was the one
//! writing the state the window reads.
//!
//! The role is what ends that: a named object the operating system hands to one
//! holder at a time. The holder is the owner - it binds the ports, it answers
//! the names, it writes the record. The others wait, and take over the moment
//! the owner goes away.
//!
//! Waiting rather than exiting is deliberate. The relay is started by a
//! scheduled task whose restart policy is "three times, a minute apart"; a
//! second relay that exited would be restarted three times and then the task
//! would give up for good, leaving the machine with no relay at all - and no
//! error to show for it. A waiter costs one process and turns into the owner
//! the instant the owner is gone, which is the failover the restart policy was
//! there to provide, without its ceiling.
//!
//! Windows' mutexes are recursive: the thread that owns one may take it again.
//! So the second attempt inside the same thread would otherwise succeed and
//! the role would be worth nothing. `HELD` is what stops that, and it also
//! gives the role the same meaning on Unix, where `flock` is not recursive at
//! all.

use std::time::{Duration, Instant};

/// The role's name. `Local\` - per user session: the relay serves that user's
/// own Antigravity, and two sessions of one user are two machines as far as
/// the bypass is concerned.
pub const RELAY_ROLE_NAME: &str = r"Local\OpenAntigravityRelayRole";

/// How long a waiter sleeps between attempts.
const POLL: Duration = Duration::from_secs(5);

/// Whether this process already holds the role.
///
/// The mutex a thread owns may be taken again by that same thread, so without
/// this the second attempt inside one process would succeed and the guard
/// would be decoration.
static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The role, held. Dropping it gives the role back.
pub struct RelayRole {
    /// Held for its side effect: ending it releases the role.
    _handle: imp::Handle,
}

impl RelayRole {
    /// Takes the role if it is free right now. `None` when somebody else holds
    /// it, or when this process already holds it.
    pub fn try_take(name: &str) -> Option<RelayRole> {
        RelayRole::take_within(name, Duration::ZERO)
    }

    /// Waits up to `timeout` for the role, and takes it when it is free.
    pub fn take_within(name: &str, timeout: Duration) -> Option<RelayRole> {
        if HELD.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let handle = imp::Handle::open(name)?;
        let deadline = Instant::now() + timeout;
        loop {
            if handle.wait(Duration::ZERO) {
                // Lost the race inside this process: give the role straight
                // back rather than hold two claims on one mutex.
                if HELD.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    return None;
                }
                return Some(RelayRole { _handle: handle });
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            std::thread::sleep(POLL.min(deadline - now));
        }
    }
}

impl Drop for RelayRole {
    fn drop(&mut self) {
        HELD.store(false, std::sync::atomic::Ordering::Release);
        // The handle releases the role on its way out.
    }
}

/// Takes the role, waiting for whoever holds it to let go.
///
/// Never returns without the role: the point of waiting is that the machine
/// keeps a relay, and a relay without the role serves nothing.
pub fn acquire_with_failover(name: &str) -> RelayRole {
    let mut said = false;
    loop {
        if let Some(role) = RelayRole::try_take(name) {
            if said {
                crate::dns_forwarder::log_proxy("релей: роль получена - выхожу в работу");
            }
            return role;
        }
        if !said {
            said = true;
            crate::dns_forwarder::log_proxy(
                "релей: роль уже занята другим релеем - жду, пока он её освободит",
            );
        }
        std::thread::sleep(POLL);
    }
}

// ---------------------------------------------------------------------------
// The operating system's object
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::time::Duration;

    extern "system" {
        fn CreateMutexW(
            lp_mutex_attributes: *mut c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> *mut c_void;
        fn WaitForSingleObject(h_object: *mut c_void, dw_milliseconds: u32) -> u32;
        fn ReleaseMutex(h_object: *mut c_void) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    /// The wait came back with the mutex.
    const WAIT_OBJECT_0: u32 = 0;
    /// The wait came back with the mutex because the thread that owned it died
    /// without releasing it. Ours now.
    const WAIT_ABANDONED: u32 = 0x0080;

    pub struct Handle(*mut c_void);

    impl Handle {
        /// Opens the named mutex. It exists whether or not anybody holds it -
        /// and holding a handle is what keeps it existing, so that a waiter
        /// does not destroy the object the owner is holding.
        pub fn open(name: &str) -> Option<Handle> {
            let mut wide: Vec<u16> = name.encode_utf16().collect();
            wide.push(0);
            // Not created with an owner: ownership is taken by the wait below,
            // and a mutex created with an initial owner would be owned by
            // whichever thread happened to create it.
            let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, wide.as_ptr()) };
            (!handle.is_null()).then_some(Handle(handle))
        }

        /// Waits for ownership. `true` when we hold it.
        pub fn wait(&self, timeout: Duration) -> bool {
            let ms = timeout.as_millis().min(u32::MAX as u128 - 1) as u32;
            let waited = unsafe { WaitForSingleObject(self.0, ms) };
            waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED
        }

        fn release(&self) {
            unsafe {
                ReleaseMutex(self.0);
                CloseHandle(self.0);
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // Releasing the mutex is what lets the next waiter take it. A
            // handle that ended without releasing would hold the role for as
            // long as the object lives - for ever, since the object only dies
            // with its last handle.
            self.release();
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use std::fs::{File, OpenOptions};
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;
    use std::time::Duration;

    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;

    pub struct Handle(File);

    impl Handle {
        pub fn open(name: &str) -> Option<Handle> {
            let path = role_path(name);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).ok()?;
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .ok()?;
            Some(Handle(file))
        }

        pub fn wait(&self, timeout: Duration) -> bool {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                if unsafe { flock(self.0.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
                    return true;
                }
                let now = std::time::Instant::now();
                if now >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(250).min(deadline - now));
            }
        }

        fn release(&self) {
            unsafe {
                flock(self.0.as_raw_fd(), LOCK_UN);
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // Unlocking is what lets the next waiter take the role.
            self.release();
        }
    }

    /// Where the role's file lives. A machine-wide, per-user place - not the
    /// portable build's own directory, which follows the executable onto a
    /// flash drive and would let two copies of the same stick both believe
    /// they own the role.
    fn role_path(name: &str) -> PathBuf {
        let stem: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })
            .unwrap_or_else(std::env::temp_dir);
        base.join("openantigravity")
            .join(format!("{stem}.role"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The take without the per-process guard.
    ///
    /// The guard makes the in-process case trivial - the second attempt is
    /// refused by the guard rather than by the operating system - so the
    /// property that matters is proved here, against the object itself: one
    /// holder at a time. That the *process* is the unit is proved across
    /// processes, in `tests/test_cli.rs`.
    #[cfg(test)]
    fn try_take_unprotected(name: &str) -> Option<imp::Handle> {
        let handle = imp::Handle::open(name)?;
        handle.wait(Duration::ZERO).then_some(handle)
    }

    /// The role is one holder at a time, and it is free again the moment the
    /// holder lets go - which is the failover the waiter relies on.
    ///
    /// The name carries the process id so a test run never collides with the
    /// relay actually running on this machine.
    #[test]
    fn the_role_is_one_holder_at_a_time_and_free_again_when_it_is_let_go() {
        let name = format!("Local\\OpenAntigravityRoleTest_{}", std::process::id());
        let first = RelayRole::try_take(&name).expect("the first holder takes the role");
        assert!(
            RelayRole::try_take(&name).is_none(),
            "a second attempt must be refused while the first holds the role"
        );
        drop(first);
        let second = RelayRole::try_take(&name).expect("the role is free once the first lets go");
        drop(second);
    }

    /// The operating system's object gives the role to one holder at a time -
    /// a second waiter is refused, and gets it once the first lets go.
    ///
    /// Two threads, not two attempts in one: a Windows mutex is recursive, so
    /// the thread that owns one may take it again. That is exactly why the
    /// per-process guard exists, and why the proof has to come from a thread
    /// that does not own it.
    #[test]
    fn the_operating_system_object_gives_the_role_to_one_holder_at_a_time() {
        let name = format!("Local\\OpenAntigravityRoleObject_{}", std::process::id());
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();

        let theirs = name.clone();
        let other = std::thread::spawn(move || {
            let held = try_take_unprotected(&theirs).expect("the other thread takes the role");
            ready_tx.send(()).ok();
            go_rx.recv().ok();
            drop(held);
        });

        ready_rx.recv().ok();
        assert!(
            try_take_unprotected(&name).is_none(),
            "the role must not be taken twice"
        );
        go_tx.send(()).ok();
        other.join().ok();
        assert!(
            try_take_unprotected(&name).is_some(),
            "the role is free once it is let go"
        );
    }
}
