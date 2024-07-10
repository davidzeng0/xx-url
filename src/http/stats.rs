use std::fmt;

use super::*;
use crate::net::conn::ConnectStats;
use crate::tls;

#[derive(Default, Clone, Copy)]
pub struct Stats {
	pub redirect: Option<Duration>,
	pub connect: Option<ConnectStats>,
	pub tls_connect: Option<Duration>,
	pub stall: Duration,
	pub wait: Duration,
	pub response: Duration
}

impl From<ConnectStats> for Stats {
	fn from(connect: ConnectStats) -> Self {
		Self { connect: Some(connect), ..Default::default() }
	}
}

impl fmt::Debug for Stats {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut stats = fmt.debug_struct("Stats");

		if let Some(connect) = &self.connect {
			stats.field("lookup", &connect.dns_resolve);
			stats.field("connect", &connect.tcp_connect);
			stats.field("tries", &connect.tcp_tries);
		}

		if let Some(tls) = &self.tls_connect {
			stats.field("tls", &tls);
		}

		stats.field("stall", &self.stall);
		stats.field("wait", &self.wait);
		stats.field("response", &self.response);
		stats.finish()
	}
}

impl From<tls::conn::ConnectStats> for Stats {
	fn from(connect: tls::conn::ConnectStats) -> Self {
		Self {
			connect: Some(connect.stats),
			tls_connect: Some(connect.tls_connect),
			..Default::default()
		}
	}
}
