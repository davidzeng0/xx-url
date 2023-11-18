use std::net::{Ipv4Addr, Ipv6Addr};

use xx_core::error::Result;
use xx_pulse::*;
use xx_url::dns::Resolver;

#[main]
#[test]
async fn test_dns() -> Result<()> {
	let resolver = Resolver::new().await?;

	let ips = resolver.resolve_ips("www.example.com").await?;

	assert_eq!(ips.v4(), &["93.184.216.34".parse::<Ipv4Addr>().unwrap()]);
	assert_eq!(
		ips.v6(),
		&["2606:2800:220:1:248:1893:25c8:1946"
			.parse::<Ipv6Addr>()
			.unwrap()]
	);

	Ok(())
}
