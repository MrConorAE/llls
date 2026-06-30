use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Watch `dir` (non-recursively); each filesystem event sends a `()` tick.
/// Keep the returned `Watcher` alive — dropping it stops the channel.
pub fn watcher(dir: &Path) -> Result<(RecommendedWatcher, Receiver<()>)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })?;
    w.watch(dir, RecursiveMode::NonRecursive)?;
    Ok((w, rx))
}

/// Block until `check()` returns `Some`, waking on filesystem events in `dir`
/// and at least every `poll` (the poll is a safety net against missed events).
/// Returns `Ok(None)` if `timeout` elapses first (`None` timeout = forever).
pub fn wait_until<T>(
    dir: &Path,
    poll: Duration,
    timeout: Option<Duration>,
    mut check: impl FnMut() -> Option<T>,
) -> Result<Option<T>> {
    if let Some(v) = check() {
        return Ok(Some(v));
    }
    let (_w, rx) = watcher(dir)?;
    let start = Instant::now();
    loop {
        if let Some(t) = timeout {
            if start.elapsed() >= t {
                return Ok(None);
            }
        }
        match rx.recv_timeout(poll) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {
                if let Some(v) = check() {
                    return Ok(Some(v));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Ok(check());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn returns_immediately_when_already_satisfied() {
        let tmp = tempfile::tempdir().unwrap();
        let got = wait_until(tmp.path(), Duration::from_millis(50), Some(Duration::from_secs(1)), || Some(7u32)).unwrap();
        assert_eq!(got, Some(7));
    }

    #[test]
    fn times_out_when_never_satisfied() {
        let tmp = tempfile::tempdir().unwrap();
        let got = wait_until(tmp.path(), Duration::from_millis(20), Some(Duration::from_millis(80)), || None::<u32>).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn observes_a_file_written_after_waiting_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("review.json");
        let t2 = target.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            std::fs::write(&t2, "ready").unwrap();
        });
        let got = wait_until(tmp.path(), Duration::from_millis(20), Some(Duration::from_secs(2)), || {
            std::fs::read_to_string(&target).ok()
        }).unwrap();
        handle.join().unwrap();
        assert_eq!(got.as_deref(), Some("ready"));
    }
}
