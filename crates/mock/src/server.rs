//! Own the loopback listener, CORS, operation admission, and shared shutdown deadline.

mod access_body;
mod connection;
mod cors;
mod drain;
mod network;
mod observations;
mod routing;
mod shutdown;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use chigiri_mock::service::{self, Worker};

pub(crate) async fn serve(config: Config, enable_graphiql: bool) -> Result<(), Diagnostic> {
    let chigiri_mock::checked::RuntimeInputs {
        server,
        scripts,
        hooks,
        store,
        clock,
        events,
        consumers,
    } = config.checked.into_runtime();
    let address = server.listen;
    let mut origins = server.allowed_origins;
    let diagnostics = config.origins;
    let executable = std::env::current_exe().unwrap_or_default();
    let mut writer = chigiri_mock::logging::Writer::start(&executable);
    let logger = writer.logger.clone();
    let mut worker = Worker::from_checked_with_events_logged(
        scripts,
        hooks,
        store,
        clock,
        events,
        consumers,
        Some(logger.clone()),
    )
    .map_err(|error| diagnostics.script(None, error))?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| Diagnostic::environment("cannot bind loopback listener"))?;
    let bound = listener
        .local_addr()
        .map_err(|_| Diagnostic::environment("cannot read bound address"))?;
    if enable_graphiql {
        let own_origin = format!("http://{bound}");
        if !origins.contains(&own_origin) {
            origins.push(own_origin);
        }
    }
    let interrupt = chigiri_host::interrupt::Interrupt::install()
        .map_err(|_| Diagnostic::environment("cannot install TERM/INT handler"))?;
    let scopes = observations::scopes(bound, logger.clone(), diagnostics.clone());
    let streaming_scopes = scopes.clone();
    let report = diagnostics.clone();
    let http_scopes =
        service::HttpScopes::new().map_err(|error| diagnostics.script(None, error))?;
    let endpoint = service::managed_endpoint(
        worker.client.clone(),
        enable_graphiql,
        move |field, error| report.script(Some(field), error).field(),
        move || streaming_scopes(true),
        http_scopes.clone(),
    )
    .map_err(|error| diagnostics.script(None, error))?;
    let maximum_connections = network::connection_capacity(&endpoint)?;
    let event_client = worker.client.clone();
    let running_client = worker.client.clone();
    let shutdown =
        shutdown::Shutdown::new(worker.client.shutdown.clone(), writer.deadline_notifier())
            .with_event_stop(
                move || event_client.quiesce_events(),
                move || running_client.events_running(),
            );
    logger.notice(&format!("chigiri mock listening http://{bound}/graphql"));
    if enable_graphiql {
        logger.notice(&format!(
            "chigiri mock GraphiQL http://{bound}/graphiql (schema introspection is public)"
        ));
    }
    let setup = routing::Routing::new(
        endpoint.clone(),
        origins,
        shutdown.clone(),
        logger.clone(),
        scopes,
        http_scopes,
    );
    let result = match setup {
        Ok(routing) => {
            network::run(
                listener,
                routing,
                endpoint,
                shutdown.clone(),
                interrupt,
                maximum_connections,
            )
            .await
        }
        Err(_) => Err(Diagnostic::environment(
            "cannot initialize managed services",
        )),
    };
    // The SDK stop policy finishes before native network retirement. Keep the
    // same remaining budget for the VM and writer, including startup failure.
    let deadline = shutdown.begin_native();
    let vm_result = worker
        .stop_graceful(deadline)
        .await
        .map_err(|error| diagnostics.script(None, error));
    if vm_result.is_ok() {
        logger.notice("chigiri mock worker joined");
    }
    let result = result.and(vm_result);
    if let Err(error) = &result {
        logger.notice(&error.to_string());
    }
    let log_result = writer.stop(deadline).await.map_err(Diagnostic::environment);
    if let Err(error) = &log_result {
        logger.notice(&error.to_string());
    }
    result.and(log_result.map(|_| ())).map_err(|mut error| {
        // The isolated writer already owns this diagnostic; do not retry synchronous stderr on the main thread.
        error.output_owned = true;
        error
    })
}
