use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use resolv_conf::Config as ResolveConfig;
use simple_dns::rdata::RData;
use simple_dns::{
	Name, Packet, PacketFlag, Question as Query, ResourceRecord as Record, SimpleDnsError,
	CLASS as DnsClass, QCLASS as QueryClass, QTYPE as QueryType, RCODE as ResponseCode,
	TYPE as RecordType
};
use xx_core::async_std::io::*;
use xx_core::async_std::AsyncIteratorExt;
use xx_core::macros::duration;

use super::*;

pub mod config;
pub mod hosts;
pub mod lookup;
pub mod name_server;
pub mod resolver;

pub use config::*;
pub use hosts::*;
pub use lookup::*;
pub use name_server::*;
pub use resolver::*;

#[errors]
#[allow(clippy::large_enum_variant, variant_size_differences)]
pub enum DnsError {
	#[display("No data")]
	#[kind = ErrorKind::NoData]
	NoData,

	#[display("No records")]
	#[kind = ErrorKind::NoData]
	NoRecords {
		queries: Vec<Query<'static>>,
		soa: Option<Record<'static>>,
		response_code: ResponseCode
	},

	#[display(transparent)]
	Other(
		#[from]
		#[source]
		SimpleDnsError
	)
}
