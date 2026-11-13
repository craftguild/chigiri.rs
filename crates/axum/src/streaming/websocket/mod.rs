//! Native upgrade responses and isolated connection ownership fixtures.

#[cfg(test)]
mod driver;
#[cfg(test)]
mod port;
pub(crate) mod response;
#[cfg(test)]
mod service;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use driver::AxumWebSocketConnectionDriver;
#[cfg(test)]
pub(super) use service::AxumWebSocketUpgradeService;

#[cfg(test)]
use chigiri::Endpoint;

#[cfg(test)]
pub(super) fn connection<C: Send + Sync + 'static>(
    endpoint: Endpoint<C>,
) -> (
    AxumWebSocketUpgradeService<C>,
    AxumWebSocketConnectionDriver<C>,
) {
    let (registration, receiver) = port::pair();
    (
        AxumWebSocketUpgradeService::new(endpoint.clone(), registration),
        AxumWebSocketConnectionDriver::new(endpoint, receiver),
    )
}
