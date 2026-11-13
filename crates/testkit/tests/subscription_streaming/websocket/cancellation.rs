//! Explicit completion and TCP loss cancel a real source, not a test future.

use std::sync::atomic::Ordering;

use super::super::{
    fixture,
    server::{Adapter, RunningServer},
};
use super::{Client, GREETING, TICKS, group, through_terminals};

pub(super) fn run(adapter: Adapter) {
    for explicit in [false, true] {
        let (endpoint, mapped, probe) = fixture::endpoint_with_pending_ticks();
        let server = RunningServer::start_websocket(adapter, endpoint);
        let mut client = Client::connect(server.address());
        client.initialize();
        client.subscribe("cancel", TICKS);
        for expected in [1, 2] {
            let message = client.next();
            assert_eq!(message["id"], "cancel");
            assert_eq!(message["type"], "next");
            assert_eq!(message["payload"]["data"]["ticks"]["value"], expected);
        }
        probe.wait_until_pending();
        if explicit {
            client.send(serde_json::json!({"id":"cancel", "type":"complete"}));
            client.subscribe("cancel", GREETING);
            let grouped = group(through_terminals(&mut client, &["cancel"]));
            assert_eq!(
                grouped.len(),
                1,
                "client completion must emit no old terminal"
            );
            assert_eq!(grouped["cancel"].len(), 2);
            assert_eq!(grouped["cancel"][0]["payload"]["data"]["greeting"], "hello");
            probe.wait_until_cancelled();
            client.close();
        }
        drop(client);
        if !explicit {
            probe.wait_until_cancelled();
        }
        server.finish();
        probe.assert_cancelled();
        assert_eq!(mapped.load(Ordering::SeqCst), 0);
    }
}
