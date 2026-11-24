//! When an instance's mpv handle may be used, and when it may be destroyed.
//!
//! mpv requires that no other API call runs on a handle while
//! `mpv_terminate_destroy` runs, and that the event thread has stopped. This
//! gate counts the calls in flight and closes in two steps:
//!
//! 1. `begin_close` (phase `Closing`): only one caller wins; it gets the event
//!    thread to join. Calls are still admitted, because callbacks running on
//!    the event thread may call back into the instance while the closer waits
//!    for that thread to finish.
//! 2. `finish_close` (phase `Closed`), after the join: new calls are refused,
//!    and it waits until the calls in flight have left.
//!
//! No caller ever blocks to *enter*: a call is admitted or refused at once.
//! The only waits are the closer's (the join, then `finish_close`), and every
//! thread it waits for holds no lock of ours while it runs (see `enter`).

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Open,
    Closing,
    Closed,
}

pub struct Lifecycle<W> {
    inner: Mutex<Inner<W>>,
    idle: Condvar,
}

struct Inner<W> {
    phase: Phase,
    active: usize,
    worker: Option<W>,
    // kept after the worker is taken, so that the worker is still recognised
    // while it is being joined
    worker_thread: Option<ThreadId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError<E> {
    /// `begin_close` already ran: the worker would never be joined.
    Closing,
    AlreadyStarted,
    Spawn(E),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CloseError {
    /// Called from the worker thread, which would have to join itself.
    FromWorker,
    /// Another caller is closing or has closed it.
    AlreadyClosing,
}

/// One call in flight. While it exists, `finish_close` does not return.
#[must_use]
pub struct Call<'a, W> {
    lifecycle: &'a Lifecycle<W>,
}

impl<W> Drop for Call<'_, W> {
    fn drop(&mut self) {
        let mut inner = self.lifecycle.lock();
        inner.active -= 1;
        if inner.active == 0 && inner.phase == Phase::Closed {
            self.lifecycle.idle.notify_all();
        }
    }
}

impl<W> Default for Lifecycle<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W> Lifecycle<W> {
    pub fn new() -> Self {
        Lifecycle {
            inner: Mutex::new(Inner {
                phase: Phase::Open,
                active: 0,
                worker: None,
                worker_thread: None,
            }),
            idle: Condvar::new(),
        }
    }

    // Every critical section below only reads or writes these fields (spawning
    // the worker aside, which does not wait for it), so a panic cannot leave
    // them inconsistent and poisoning can be ignored.
    fn lock(&self) -> MutexGuard<'_, Inner<W>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Admits a call, or refuses it (`None`) once the instance is `Closed`.
    /// Never blocks beyond the brief internal lock. The caller must not hold
    /// the returned `Call` while running Java callbacks or waiting for another
    /// thread: `finish_close` waits for it.
    pub fn enter(&self) -> Option<Call<'_, W>> {
        let mut inner = self.lock();
        if inner.phase == Phase::Closed {
            return None;
        }
        inner.active += 1;
        Some(Call { lifecycle: self })
    }

    /// Starts the worker with `spawn` and records it, unless closing has begun
    /// or a worker exists. `spawn` runs under the internal lock so that a
    /// concurrent `begin_close` sees either no worker and the refusal here, or
    /// the worker; it must not wait for the worker to do anything.
    pub fn start_worker<E>(
        &self,
        spawn: impl FnOnce() -> Result<(W, ThreadId), E>,
    ) -> Result<(), StartError<E>> {
        let mut inner = self.lock();
        if inner.phase != Phase::Open {
            return Err(StartError::Closing);
        }
        if inner.worker_thread.is_some() {
            return Err(StartError::AlreadyStarted);
        }
        let (worker, id) = spawn().map_err(StartError::Spawn)?;
        inner.worker = Some(worker);
        inner.worker_thread = Some(id);
        Ok(())
    }

    #[cfg(test)]
    pub fn is_worker_thread(&self) -> bool {
        self.lock().worker_thread == Some(thread::current().id())
    }

    #[cfg(test)]
    pub fn phase(&self) -> Phase {
        self.lock().phase
    }

    /// Step 1 of closing. Returns the worker for the caller to stop and join
    /// (`None` if none was started). Exactly one caller gets `Ok`.
    pub fn begin_close(&self) -> Result<Option<W>, CloseError> {
        let mut inner = self.lock();
        if inner.worker_thread == Some(thread::current().id()) {
            return Err(CloseError::FromWorker);
        }
        if inner.phase != Phase::Open {
            return Err(CloseError::AlreadyClosing);
        }
        inner.phase = Phase::Closing;
        Ok(inner.worker.take())
    }

    /// Step 2 of closing, for the caller that won `begin_close`, after it
    /// joined the worker: refuses new calls and returns once none is in
    /// flight. From then on the caller has the handle to itself.
    pub fn finish_close(&self) {
        let mut inner = self.lock();
        debug_assert_eq!(inner.phase, Phase::Closing);
        inner.phase = Phase::Closed;
        while inner.active > 0 {
            inner = self
                .idle
                .wait(inner)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    type Lc = Lifecycle<JoinHandle<()>>;

    #[test]
    fn calls_are_admitted_until_closed() {
        let lc: Lc = Lifecycle::new();
        assert_eq!(lc.phase(), Phase::Open);
        drop(lc.enter().expect("open"));
        assert!(lc.begin_close().unwrap().is_none(), "no worker was started");
        assert_eq!(lc.phase(), Phase::Closing);
        drop(lc.enter().expect("still admitted while closing"));
        lc.finish_close();
        assert_eq!(lc.phase(), Phase::Closed);
        assert!(lc.enter().is_none());
    }

    #[test]
    fn second_close_is_refused() {
        let lc: Lc = Lifecycle::new();
        assert!(lc.begin_close().is_ok());
        assert_eq!(lc.begin_close().unwrap_err(), CloseError::AlreadyClosing);
        lc.finish_close();
        assert_eq!(lc.begin_close().unwrap_err(), CloseError::AlreadyClosing);
    }

    #[test]
    fn concurrent_closes_have_exactly_one_winner() {
        for _ in 0..50 {
            let lc: Arc<Lc> = Arc::new(Lifecycle::new());
            let winners = Arc::new(AtomicUsize::new(0));
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    let lc = Arc::clone(&lc);
                    let winners = Arc::clone(&winners);
                    thread::spawn(move || {
                        if lc.begin_close().is_ok() {
                            winners.fetch_add(1, Ordering::SeqCst);
                            lc.finish_close();
                        }
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            assert_eq!(winners.load(Ordering::SeqCst), 1);
            assert_eq!(lc.phase(), Phase::Closed);
        }
    }

    #[test]
    fn finish_close_waits_for_calls_in_flight() {
        let lc: Arc<Lc> = Arc::new(Lifecycle::new());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let left = Arc::new(AtomicBool::new(false));

        let caller = {
            let lc = Arc::clone(&lc);
            let left = Arc::clone(&left);
            thread::spawn(move || {
                let call = lc.enter().unwrap();
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                left.store(true, Ordering::SeqCst);
                drop(call);
            })
        };
        entered_rx.recv().unwrap();

        lc.begin_close().unwrap();
        let closer = {
            let lc = Arc::clone(&lc);
            let left = Arc::clone(&left);
            thread::spawn(move || {
                lc.finish_close();
                assert!(
                    left.load(Ordering::SeqCst),
                    "returned with a call in flight"
                );
            })
        };
        // the closer is now waiting, or about to; new calls are refused soon
        while lc.phase() != Phase::Closed {
            thread::yield_now();
        }
        assert!(lc.enter().is_none());
        thread::sleep(Duration::from_millis(20));
        assert!(!closer.is_finished());
        release_tx.send(()).unwrap();
        closer.join().unwrap();
        caller.join().unwrap();
    }

    #[test]
    fn worker_may_call_in_while_the_closer_joins_it() {
        // The shape of a destroy() racing a callback that calls back into the
        // instance: the closer joins the worker without holding the lock, and
        // the worker's calls are admitted until the join is done.
        let lc: Arc<Lc> = Arc::new(Lifecycle::new());
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (running_tx, running_rx) = mpsc::channel();
        {
            let lc2 = Arc::clone(&lc);
            let stop = Arc::clone(&stop);
            let calls = Arc::clone(&calls);
            lc.start_worker(|| -> Result<_, ()> {
                let h = thread::spawn(move || {
                    running_tx.send(()).unwrap();
                    while !stop.load(Ordering::SeqCst) {
                        let call = lc2.enter().expect("admitted until the worker is joined");
                        calls.fetch_add(1, Ordering::SeqCst);
                        drop(call);
                        thread::yield_now();
                    }
                    // a few more after the stop request, as a callback in
                    // progress would
                    for _ in 0..10 {
                        drop(lc2.enter().expect("still admitted"));
                    }
                });
                let id = h.thread().id();
                Ok((h, id))
            })
            .unwrap();
        }
        running_rx.recv().unwrap();

        let worker = lc.begin_close().unwrap().expect("a worker was started");
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        lc.finish_close();
        assert!(lc.enter().is_none());
        assert!(calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn close_from_the_worker_is_refused_and_leaves_it_open() {
        let lc: Arc<Lc> = Arc::new(Lifecycle::new());
        let (tx, rx) = mpsc::channel();
        {
            let lc2 = Arc::clone(&lc);
            lc.start_worker(|| -> Result<_, ()> {
                let h = thread::spawn(move || {
                    // wait until start_worker has recorded us
                    while !lc2.is_worker_thread() {
                        thread::yield_now();
                    }
                    tx.send(lc2.begin_close()).unwrap();
                    // still usable from here
                    drop(lc2.enter().expect("open"));
                });
                let id = h.thread().id();
                Ok((h, id))
            })
            .unwrap();
        }
        assert_eq!(rx.recv().unwrap().unwrap_err(), CloseError::FromWorker);
        assert_eq!(lc.phase(), Phase::Open);
        let worker = lc.begin_close().unwrap().unwrap();
        worker.join().unwrap();
        lc.finish_close();
    }

    #[test]
    fn close_from_the_worker_is_refused_while_another_thread_closes() {
        let lc: Arc<Lc> = Arc::new(Lifecycle::new());
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let (result_tx, result_rx) = mpsc::channel();
        {
            let lc2 = Arc::clone(&lc);
            lc.start_worker(|| -> Result<_, ()> {
                let h = thread::spawn(move || {
                    started_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                    result_tx.send(lc2.begin_close()).unwrap();
                });
                let id = h.thread().id();
                Ok((h, id))
            })
            .unwrap();
        }
        started_rx.recv().unwrap();
        let worker = lc.begin_close().unwrap().unwrap();
        go_tx.send(()).unwrap();
        assert_eq!(
            result_rx.recv().unwrap().unwrap_err(),
            CloseError::FromWorker
        );
        worker.join().unwrap();
        lc.finish_close();
    }

    #[test]
    fn worker_is_not_started_once_closing() {
        let lc: Lc = Lifecycle::new();
        lc.begin_close().unwrap();
        let spawned = AtomicBool::new(false);
        let r = lc.start_worker(|| -> Result<(JoinHandle<()>, ThreadId), ()> {
            spawned.store(true, Ordering::SeqCst);
            unreachable!()
        });
        assert_eq!(r.unwrap_err(), StartError::Closing);
        assert!(!spawned.load(Ordering::SeqCst));
        lc.finish_close();
    }

    #[test]
    fn worker_starts_once_and_spawn_errors_are_reported() {
        let lc: Lifecycle<u32> = Lifecycle::new();
        assert_eq!(
            lc.start_worker(|| Err::<(u32, ThreadId), _>("no threads")),
            Err(StartError::Spawn("no threads"))
        );
        // a failed spawn leaves no worker behind, so it may be tried again
        let id = thread::current().id();
        assert_eq!(lc.start_worker(|| Ok::<_, ()>((7, id))), Ok(()));
        assert_eq!(
            lc.start_worker(|| Ok::<_, ()>((8, id))),
            Err(StartError::AlreadyStarted)
        );
        // (this thread is now the recorded worker, so close from another one)
        let lc = Arc::new(lc);
        let lc2 = Arc::clone(&lc);
        let w = thread::spawn(move || lc2.begin_close()).join().unwrap();
        assert_eq!(w, Ok(Some(7)));
    }
}
