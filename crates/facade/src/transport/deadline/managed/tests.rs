//! Original shorter budgets retain their status after a delayed outer poll.

use super::*;

struct Deadlines;
impl DeadlineFactory for Deadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
static DEADLINES: Deadlines = Deadlines;

#[test]
fn delayed_timer_observation_preserves_the_shorter_body_or_overall_budget() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for (overall, phase, complete_phase, status) in [
            (60, 20, false, 408),
            (20, 60, false, 504),
            (60, 20, true, 504),
        ] {
            let now = tokio::time::Instant::now();
            let (mut state, monitor) = HttpDeadlineState::new(
                &DEADLINES,
                now + Duration::from_millis(overall),
                Control::new(),
                None,
            )
            .unwrap();
            let clock = monitor.managed.as_ref().unwrap();
            let proxy = clock
                .armer
                .arm_http_phase(
                    Duration::from_millis(phase),
                    HttpPhaseDeadline {
                        kind: LifecyclePhase::BodyRead,
                        cutoff: now + Duration::from_millis(phase),
                    },
                )
                .unwrap();
            if complete_phase {
                proxy.clear().unwrap();
            } else {
                proxy.relinquish();
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
            let result = std::future::poll_fn(|cx| {
                state.poll_timeout(cx, ResponseMediaType::GraphQlResponseJson)
            })
            .await
            .unwrap();
            assert_eq!(result.status, status);
        }
    });
}
