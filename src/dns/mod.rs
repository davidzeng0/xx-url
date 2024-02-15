use std::{
	collections::HashMap,
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
	str::FromStr,
	time::{Duration, Instant}
};

use hickory_proto::{
	error::ProtoError,
	op::*,
	rr::{rdata::SOA, resource::RecordRef, *},
	serialize::binary::BinDecodable,
	xfer::DnsResponse
};
use resolv_conf::Config as ResolveConfig;
use thiserror::Error;
use xx_core::{
	async_std::{io::*, AsyncIteratorExt},
	macros::duration
};

use super::*;

mod config;
pub use config::*;
mod hosts;
pub use hosts::*;
mod lookup;
pub use lookup::*;
mod name_server;
pub use name_server::*;
mod resolver;
pub use resolver::*;

#[derive(Debug, Error)]
pub enum DnsError {
	#[error("No data")]
	NoData,

	#[error("No records for {:?}", query)]
	NoRecords {
		query: Query,
		soa: Option<Record<SOA>>,
		response_code: ResponseCode
	},

	#[error("Proto error")]
	Proto(#[from] ProtoError)
}
