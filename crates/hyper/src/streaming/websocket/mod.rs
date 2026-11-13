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
pub(super) use driver::HyperWebSocketConnectionDriver;
#[cfg(test)]
pub(super) use service::HyperWebSocketUpgradeService;

#[cfg(test)]
use chigiri::Endpoint;

#[cfg(test)]
pub(super) fn connection<C: Send + Sync + 'static>(
    endpoint: Endpoint<C>,
) -> (
    HyperWebSocketUpgradeService<C>,
    HyperWebSocketConnectionDriver<C>,
) {
    let (registration, receiver) = port::pair();
    (
        HyperWebSocketUpgradeService::new(endpoint.clone(), registration),
        HyperWebSocketConnectionDriver::new(endpoint, receiver),
    )
}
