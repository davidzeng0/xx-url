use std::time::Duration;

use resolv_conf::Config as ResolveConfig;
use xx_core::{async_std::io::*, error::*};
use xx_pulse::*;

use super::name_server::NameServer;
use crate::env::resolver_conf_path;

pub struct Config {
	pub name_servers: Vec<NameServer>,
	pub ndots: u32,
	pub attempts: u32,
	pub rotate: bool,
	pub timeout: Duration
}

#[async_fn]
impl Config {
	pub async fn new() -> Result<Self> {
		let mut data = Vec::new();

		BufReader::new(File::open(resolver_conf_path()).await?)
			.read_to_end(&mut data)
			.await?;

		let config = ResolveConfig::parse(data).map_err(Error::invalid_data_error)?;

		let mut name_servers = Vec::new();

		for ip in &config.nameservers {
			name_servers.push(NameServer::new(ip.into()));
		}

		Ok(Config {
			name_servers,
			ndots: config.ndots,
			attempts: config.attempts,
			rotate: config.rotate,
			timeout: Duration::from_secs(config.timeout as u64)
		})
	}
}
