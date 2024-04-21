#![allow(warnings)]

use std::net::{Ipv4Addr, Ipv6Addr};

use xx_core::error::Result;
use xx_pulse::*;
use xx_url::dns::Resolver;

#[main]
#[test]
async fn test_dns() -> Result<()> {
	let resolver = Resolver::new().await?;

	let ips = resolver.resolve_ips("www.example.com").await?;

	assert_eq!(ips.v4(), &["93.184.215.14".parse::<Ipv4Addr>().unwrap()]);
	assert_eq!(
		ips.v6(),
		&["2606:2800:21f:cb07:6820:80da:af6b:8b2c"
			.parse::<Ipv6Addr>()
			.unwrap()]
	);

	Ok(())
}
