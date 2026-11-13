#![deny(warnings)]

#[cfg(test)]
pub use ::generated;

#[cfg(test)]
mod direct;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod governance;
#[cfg(test)]
mod http;
