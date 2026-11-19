use anyhow::Result;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{sleep, spawn};
use std::time::Duration;

/// How long a server gets to exit on SIGTERM before it gets SIGKILL.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(2);

/// Tracks whether the run was cancelled and which server process groups are
/// running, so that a signal can unblock a runner waiting on a slow fixture.
#[derive(Default)]
pub struct ServerControl {
    cancelled: AtomicBool,
    process_groups: Mutex<Vec<i32>>,
}

impl ServerControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Stops the run and asks the running servers to exit.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.signal(libc::SIGTERM);
    }

    pub fn attach(&self, process_group: i32) {
        self.lock().push(process_group);
        if self.is_cancelled() {
            signal_group(process_group, libc::SIGTERM);
        }
    }

    pub fn detach(&self, process_group: i32) {
        self.lock().retain(|attached| *attached != process_group);
    }

    /// Signals every running server.
    pub fn signal(&self, signal: i32) {
        for process_group in self.lock().iter() {
            signal_group(*process_group, signal);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<i32>> {
        self.process_groups
            .lock()
            .expect("server control lock poisoned")
    }
}

/// Signals one server and everything it started.
pub fn signal_group(process_group: i32, signal: i32) {
    // SAFETY: killpg takes no pointers. An error only means that the process
    // group has already gone.
    unsafe {
        libc::killpg(process_group, signal);
    }
}

/// Turns SIGTERM and SIGINT into a cancelled run.
pub fn cancel_on_signals(control: Arc<ServerControl>) -> Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    spawn(move || {
        for _ in signals.forever() {
            control.cancel();
            // The runner may be blocked reading from a server that ignores
            // SIGTERM. Only the death of that server releases it.
            sleep(TERMINATE_GRACE);
            control.signal(libc::SIGKILL);
        }
    });
    Ok(())
}
