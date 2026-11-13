use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::client::{drain_after_pressure, request, send_without_reading};
use super::fixture;
use super::server::{Adapter, RunningServer};
use super::validation::{
    assert_finite, assert_multipart_error, assert_multipart_ticks, assert_policy_terminal,
    assert_selected_error, assert_sse_error, assert_sse_ticks,
};

const TICKS: &str = "subscription { ticks { value } }";
const FAILING_TICKS: &str = "subscription { failingTicks { value } }";
const PARSE_ERROR: &str = "subscription {";
const VALIDATION_ERROR: &str = "subscription { missing { value } }";
const GREETING: &str = "query { greeting }";

#[derive(Clone, Copy)]
enum Profile {
    ServerSentEvents,
    Multipart,
}

impl Profile {
    const fn accept(self) -> &'static str {
        match self {
            Self::ServerSentEvents => "text/event-stream",
            Self::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        }
    }

    fn assert_ticks(self, response: &super::client::HttpResponse) {
        match self {
            Self::ServerSentEvents => assert_sse_ticks(response),
            Self::Multipart => assert_multipart_ticks(response),
        }
    }

    fn assert_error(self, response: &super::client::HttpResponse) {
        match self {
            Self::ServerSentEvents => assert_sse_error(response),
            Self::Multipart => assert_multipart_error(response),
        }
    }
}

pub fn run() {
    for adapter in [Adapter::Hyper, Adapter::Axum] {
        for profile in [Profile::ServerSentEvents, Profile::Multipart] {
            ordered_completion(adapter, profile);
            source_error_after_event(adapter, profile);
            selected_error(
                adapter,
                profile,
                VALIDATION_ERROR,
                "GRAPHQL_VALIDATION_FAILED",
                0,
            );
            initializer_error(adapter, profile);
            source_error_before_event(adapter, profile);
            unread_disconnect(adapter, profile);
            unread_slow_consumer(adapter, profile);
        }
        finite_query(adapter);
        super::websocket::run(adapter);
    }
    for adapter in [Adapter::Hyper, Adapter::Axum] {
        for profile in [Profile::ServerSentEvents, Profile::Multipart] {
            selected_error(adapter, profile, PARSE_ERROR, "GRAPHQL_PARSE_FAILED", 0);
        }
    }
}

fn ordered_completion(adapter: Adapter, profile: Profile) {
    let (server, mapped_errors) = start(adapter);
    profile.assert_ticks(&request(server.address(), profile.accept(), TICKS));
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 0);
}

fn selected_error(
    adapter: Adapter,
    profile: Profile,
    document: &str,
    expected_code: &str,
    expected_mapped_errors: usize,
) {
    let (server, mapped_errors) = start(adapter);
    assert_selected_error(
        profile.accept(),
        &request(server.address(), profile.accept(), document),
        expected_code,
    );
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), expected_mapped_errors);
}

fn source_error_after_event(adapter: Adapter, profile: Profile) {
    let (server, mapped_errors) = start(adapter);
    profile.assert_error(&request(server.address(), profile.accept(), FAILING_TICKS));
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 1);
}

fn source_error_before_event(adapter: Adapter, profile: Profile) {
    let (endpoint, mapped_errors) = fixture::endpoint_with_immediate_source_error();
    let server = RunningServer::start(adapter, endpoint);
    assert_selected_error(
        profile.accept(),
        &request(server.address(), profile.accept(), FAILING_TICKS),
        "SOURCE_FAILED",
    );
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 1);
}

fn initializer_error(adapter: Adapter, profile: Profile) {
    let (endpoint, mapped_errors) = fixture::endpoint_with_initializer_error();
    let server = RunningServer::start(adapter, endpoint);
    assert_selected_error(
        profile.accept(),
        &request(server.address(), profile.accept(), FAILING_TICKS),
        "INITIALIZER_DENIED",
    );
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 1);
}

fn unread_disconnect(adapter: Adapter, profile: Profile) {
    let (endpoint, mapped_errors, probe) = fixture::endpoint_with_pending_ticks();
    let server = RunningServer::start_disconnect(adapter, endpoint);
    let stream = send_without_reading(server.address(), profile.accept(), TICKS);
    probe.wait_until_pending();
    drop(stream);
    probe.wait_until_cancelled();
    server.finish();
    probe.assert_cancelled();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 0);
}

fn finite_query(adapter: Adapter) {
    let (server, mapped_errors) = start(adapter);
    assert_finite(&request(
        server.address(),
        "application/graphql-response+json",
        GREETING,
    ));
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 0);
}

fn unread_slow_consumer(adapter: Adapter, profile: Profile) {
    let (endpoint, mapped_errors, probe) = fixture::endpoint_with_flood();
    let server = RunningServer::start_pressure(adapter, endpoint);
    let mut stream = send_without_reading(
        server.address(),
        profile.accept(),
        "subscription { ticks { value padding } }",
    );
    probe.wait_until_pressure_cancels();
    // Reading resumes only after cancellation. The peer remains connected
    // until the actual policy terminal and successful HTTP join are verified.
    assert_policy_terminal(profile.accept(), &drain_after_pressure(&mut stream));
    server.wait_for_connection();
    drop(stream);
    server.finish();
    assert_eq!(mapped_errors.load(Ordering::SeqCst), 0);
}

fn start(adapter: Adapter) -> (RunningServer, Arc<AtomicUsize>) {
    let (endpoint, mapped_errors) = fixture::endpoint();
    (RunningServer::start(adapter, endpoint), mapped_errors)
}
