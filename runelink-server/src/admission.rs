//! Lets a connection's read loop wait until a request has been queued where it
//! runs, rather than until it finishes.
//!
//! Requests from one connection then reach each actor in the order they
//! arrived, while requests waiting on a peer host's reply never hold up the
//! requests behind them. Requests to different actors, even a channel's and
//! its server's, may still run in either order.

use std::cell::Cell;

use tokio::sync::oneshot;

tokio::task_local! {
    static ADMISSION: Cell<Option<oneshot::Sender<()>>>;
}

/// Wrap a request, returning it along with a future that resolves once the
/// request is admitted: queued on an actor, sent to a peer host, or finished.
pub fn track<F: Future>(
    request: F,
) -> (impl Future<Output = F::Output>, impl Future<Output = ()>) {
    let (admitted, wait) = oneshot::channel();
    let request = ADMISSION.scope(Cell::new(Some(admitted)), request);
    (request, async move {
        // Also resolves when the request finishes without being admitted.
        let _ = wait.await;
    })
}

/// Run part of a request without admitting it, for work that must finish
/// before the request is queued on its actor, even if it waits on a peer.
pub async fn hold<F: Future>(work: F) -> F::Output {
    let admitted = ADMISSION.try_with(Cell::take).ok().flatten();
    let output = work.await;
    if let Some(admitted) = admitted {
        let _ = ADMISSION.try_with(|admission| admission.set(Some(admitted)));
    }
    output
}

/// Mark the current request as admitted. Later calls do nothing.
pub fn admit() {
    let _ = ADMISSION.try_with(|admission| {
        if let Some(admitted) = admission.take() {
            let _ = admitted.send(());
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn admission_resolves_before_the_request_finishes() {
        let (finish, finished) = oneshot::channel::<()>();
        let (request, admitted) = track(async move {
            admit();
            admit();
            let _ = finished.await;
        });
        let request = tokio::spawn(request);
        tokio::time::timeout(Duration::from_secs(5), admitted)
            .await
            .expect("request was admitted");
        assert!(!request.is_finished());
        finish.send(()).unwrap();
        request.await.unwrap();
    }

    #[tokio::test]
    async fn requests_that_are_never_admitted_resolve_when_they_finish() {
        let (request, admitted) = track(async {});
        tokio::spawn(request);
        tokio::time::timeout(Duration::from_secs(5), admitted)
            .await
            .expect("finished request counts as admitted");
        // Outside of a tracked request, admitting does nothing.
        admit();
    }

    #[tokio::test]
    async fn held_work_does_not_admit() {
        let (resume, resumed) = oneshot::channel::<()>();
        let (request, admitted) = track(async move {
            hold(async { admit() }).await;
            let _ = resumed.await;
            admit();
        });
        tokio::spawn(request);
        let mut admitted = Box::pin(admitted);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut admitted)
                .await
                .is_err()
        );
        resume.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), admitted)
            .await
            .expect("admitted after the held work");
    }
}
