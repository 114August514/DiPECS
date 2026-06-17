//! Daemon process lifecycle for the `dipecsd` binary.
//!
//! This stays in `aios-daemon` because daemonization and signal wiring are
//! executable runtime concerns, not collection, decision, or action logic.

use std::{io, process};

/// Move the current process into daemon mode with `fork` + `setsid`.
///
/// This is only active on Linux. Other platforms keep running in the
/// foreground so local development remains simple.
pub fn daemonize() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: The binary calls POSIX daemon setup before spawning worker
        // tasks. The parent exits immediately; the child becomes a session
        // leader and redirects stdio to /dev/null.
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                tracing::error!("fork failed: {}", std::io::Error::last_os_error());
                process::exit(1);
            }
            if pid > 0 {
                process::exit(0);
            }

            if libc::setsid() < 0 {
                tracing::error!("setsid failed: {}", std::io::Error::last_os_error());
                process::exit(1);
            }

            if libc::chdir(c"/".as_ptr()) < 0 {
                tracing::error!("chdir failed: {}", io::Error::last_os_error());
                process::exit(1);
            }

            if redirect_stdio_to_dev_null() < 0 {
                tracing::error!("stdio redirect failed: {}", io::Error::last_os_error());
                process::exit(1);
            }
        }
    }

    tracing::info!("dipecsd daemon started (pid={})", process::id());
}

#[cfg(target_os = "linux")]
unsafe fn redirect_stdio_to_dev_null() -> libc::c_int {
    for fd in 0..=2 {
        // SAFETY: close is called with the standard fd numbers only.
        let _ = unsafe { libc::close(fd) };
    }

    // SAFETY: opening the constant /dev/null path is the POSIX stdio redirect primitive.
    let null_fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR) };
    if null_fd != 0 {
        return -1;
    }

    for expected_fd in 1..=2 {
        // SAFETY: fd 0 is the opened /dev/null descriptor, dup returns the next stdio fd.
        let fd = unsafe { libc::dup(0) };
        if fd != expected_fd {
            return -1;
        }
    }
    0
}

/// Install SIGINT/SIGTERM handlers and return a shutdown receiver.
pub fn install_signal_handlers() -> tokio::sync::broadcast::Receiver<()> {
    let (tx, rx) = tokio::sync::broadcast::channel::<()>(1);

    let tx_ctrlc = tx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("received SIGINT, initiating graceful shutdown");
            let _ = tx_ctrlc.send(());
        }
    });

    #[cfg(unix)]
    {
        let tx_term = tx.clone();
        tokio::spawn(async move {
            if let Ok(mut sigterm) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                sigterm.recv().await;
                tracing::info!("received SIGTERM, initiating graceful shutdown");
                let _ = tx_term.send(());
            }
        });
    }

    rx
}
