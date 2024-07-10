use super::*;

pub struct Config {
	pub name_servers: Vec<NameServer>,
	pub ndots: u32,
	pub attempts: u32,
	pub rotate: bool,
	pub timeout: Duration
}

#[asynchronous]
impl Config {
	pub async fn new() -> Result<Self> {
		let data = fs::read(resolver_conf_path()).await?;
		let config = ResolveConfig::parse(data).map_err(Error::new)?;

		let mut name_servers = Vec::new();

		for ip in &config.nameservers {
			name_servers.push(NameServer::new(ip.into()));
		}

		Ok(Self {
			name_servers,
			ndots: config.ndots,
			attempts: config.attempts,
			rotate: config.rotate,
			timeout: Duration::from_secs(config.timeout as u64)
		})
	}
}
