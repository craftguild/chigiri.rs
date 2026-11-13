//! Drive actual FIFO request commits through the bounded publication logger.

use std::time::{Duration, Instant};

use chigiri::Object;

use super::publication_tests::{create, execution, field, fixture};
use crate::logging::test_support::SubscriptionLogs;

#[tokio::test]
async fn committed_publication_logs_are_ordered_once_and_keep_no_private_values() {
    let _case = crate::vm_gate::test_case();
    for lost in [false, true] {
        let mut worker = fixture();
        let execution = execution(&worker.client);
        let logs = SubscriptionLogs::new();
        let client = worker
            .client
            .request_client(true, &execution)
            .await
            .unwrap();
        create(&client, &execution, "PRIVATE_MODEL_KEY").await;
        client
            .execute_with(
                field(&client, "Mutation", "second"),
                Object::new(),
                None,
                None,
                execution.clone(),
            )
            .await
            .unwrap()
            .output
            .unwrap();
        assert!(logs.take().is_empty());
        client.prepare_request(execution.deadline).await.unwrap();
        assert!(logs.take().is_empty());
        if lost {
            let staging = worker.client.committed.lock().unwrap().staging();
            staging.close().unwrap();
        }
        // No ambient HTTP task-local scope exists here. The explicit fresh
        // request ID and concrete logger also serve WebSocket one-shot commits.
        client.commit_request_logged(73, &logs.logger).unwrap();
        assert_eq!(
            worker.client.committed.lock().unwrap().snapshot().size().0,
            1
        );
        let records = logs.take();
        assert_eq!(records.len(), 2);
        for (index, coordinate) in ["Subscription.changed", "Subscription.event"]
            .into_iter()
            .enumerate()
        {
            let record = &records[index];
            assert_eq!(record.as_object().unwrap().len(), 8);
            assert_eq!(record["format"], "chigiri-mock-log/v5");
            assert_eq!(record["kind"], "subscription_publication");
            assert_eq!(record["request_id"], 73);
            assert_eq!(record["sequence"], 1);
            assert_eq!(record["index"], index);
            assert_eq!(record["coordinate"], coordinate);
            assert_eq!(record["outcome"], if lost { "lost" } else { "staged" });
            assert!(!record.to_string().contains("PRIVATE_MODEL_KEY"));
            assert!(!record.to_string().contains("created:"));
        }
        client.commit_request_logged(73, &logs.logger).unwrap();
        assert!(
            logs.take().is_empty(),
            "idempotent commit cannot retry logs"
        );
        drop(client);
        worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn rollback_and_requests_without_publications_have_no_publication_record() {
    let _case = crate::vm_gate::test_case();
    for rollback in [false, true] {
        let mut worker = fixture();
        let execution = execution(&worker.client);
        let logs = SubscriptionLogs::new();
        let client = worker
            .client
            .request_client(rollback, &execution)
            .await
            .unwrap();
        if rollback {
            create(&client, &execution, "discarded").await;
            execution.cancel.cancel();
            assert!(client.prepare_request(execution.deadline).await.is_err());
            assert!(client.commit_request_logged(74, &logs.logger).is_err());
        } else {
            client.prepare_request(execution.deadline).await.unwrap();
            client.commit_request_logged(74, &logs.logger).unwrap();
        }
        assert!(logs.take().is_empty());
        assert_eq!(
            worker.client.committed.lock().unwrap().snapshot().size().0,
            0
        );
        drop(client);
        worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}
