//! A small, bounded pool of store sessions for API requests. Requests wait for
//! a free session instead of opening one each, so a burst of drawer reads
//! can't exhaust Postgres connections.

use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Semaphore;

pub(crate) struct Sessions<T> {
    idle: Mutex<Vec<T>>,
    permits: Semaphore,
}

impl<T: Send + 'static> Sessions<T> {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            idle: Mutex::new(Vec::new()),
            permits: Semaphore::new(limit),
        })
    }

    /// Run blocking `work` on an idle session, opening one if none is idle.
    /// A session whose work failed is dropped, so a broken one isn't reused.
    pub(crate) async fn run<R, E>(
        self: &Arc<Self>,
        open: impl FnOnce() -> Result<T, E> + Send + 'static,
        work: impl FnOnce(&mut T) -> Result<R, E> + Send + 'static,
    ) -> Result<R, E>
    where
        R: Send + 'static,
        E: Send + 'static,
    {
        let permit = self
            .permits
            .acquire()
            .await
            .expect("the session semaphore is never closed");
        let sessions = self.clone();
        let done = tokio::task::spawn_blocking(move || {
            let idle = sessions
                .idle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop();
            let mut session = match idle {
                Some(session) => session,
                None => open()?,
            };
            let result = work(&mut session);
            if result.is_ok() {
                sessions
                    .idle
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(session);
            }
            result
        })
        .await;
        drop(permit);
        match done {
            Ok(result) => result,
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(error) => panic!("a session task was cancelled: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Sessions;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sessions_stay_within_their_bound_and_are_reused() {
        let sessions = Sessions::<usize>::new(3);
        let opened = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let runs: Vec<_> = (0..12)
            .map(|_| {
                let (sessions, opened, active, most) = (
                    sessions.clone(),
                    opened.clone(),
                    active.clone(),
                    most.clone(),
                );
                tokio::spawn(async move {
                    sessions
                        .run(
                            move || Ok::<_, ()>(opened.fetch_add(1, Ordering::SeqCst)),
                            move |_| {
                                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                                most.fetch_max(now, Ordering::SeqCst);
                                // Overlap the runs; the bound holds whatever the timing.
                                std::thread::sleep(Duration::from_millis(20));
                                active.fetch_sub(1, Ordering::SeqCst);
                                Ok(())
                            },
                        )
                        .await
                })
            })
            .collect();
        for run in runs {
            run.await.unwrap().unwrap();
        }
        assert!(
            most.load(Ordering::SeqCst) <= 3,
            "at most 3 sessions at once"
        );
        assert!(
            opened.load(Ordering::SeqCst) <= 3,
            "idle sessions are reused"
        );
    }

    #[tokio::test]
    async fn a_failed_session_is_not_reused() {
        let sessions = Sessions::<usize>::new(1);
        let opened = Arc::new(AtomicUsize::new(0));
        let open =
            |opened: Arc<AtomicUsize>| move || Ok::<_, ()>(opened.fetch_add(1, Ordering::SeqCst));
        assert_eq!(
            sessions
                .run(open(opened.clone()), |_| Err::<(), _>(()))
                .await,
            Err(())
        );
        assert_eq!(
            sessions
                .run(open(opened.clone()), |session| Ok(*session))
                .await,
            Ok(1)
        );
        assert_eq!(
            sessions
                .run(open(opened.clone()), |session| Ok(*session))
                .await,
            Ok(1)
        );
        assert_eq!(opened.load(Ordering::SeqCst), 2);
    }
}
