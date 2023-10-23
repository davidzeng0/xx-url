use std::{
	io::{IoSlice, IoSliceMut},
	net::{IpAddr, SocketAddr},
	os::fd::AsRawFd,
	sync::Arc,
	time::{Duration, Instant}
};

use enumflags2::{make_bitflags, BitFlags};
use xx_async_runtime::Context;
use xx_core::{
	async_std::io::*,
	coroutines::{async_trait_fn, check_interrupt},
	debug,
	error::*,
	os::{
		inet::IpProtocol,
		poll::{poll, PollFd, PollFlag},
		socket::{Shutdown, SocketType}
	}
};
use xx_pulse::*;

use crate::{
	dns::resolver::{LookupIp, Resolver},
	env::get_resolver
};

#[derive(Default, Clone)]
pub struct ConnectStats {
	pub dns_resolve: Duration,
	pub tcp_tries: u32,
	pub tcp_connect: Duration
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IpStrategy {
	#[default]
	Default = 0,
	Ipv4Only,
	Ipv6Only,
	PreferIpv4,
	PreferIpv6
}

pub struct ConnectOptions<'a> {
	resolver: Arc<Resolver>,
	host: &'a str,
	port: u16,
	strategy: IpStrategy,
	timeout: Option<Duration>,
	recvbuf_size: Option<i32>,
	sendbuf_size: Option<i32>,
	tcp_nodelay: bool,
	tcp_keepalive: Option<i32>
}

impl<'a> ConnectOptions<'a> {
	pub fn new(host: &'a str, port: u16) -> Self {
		Self::with_resolver(get_resolver(), host, port)
	}

	pub fn with_resolver(resolver: Arc<Resolver>, host: &'a str, port: u16) -> Self {
		Self {
			resolver,
			host,
			port,
			strategy: IpStrategy::Default,
			timeout: None,

			recvbuf_size: None,
			sendbuf_size: None,
			tcp_nodelay: false,
			tcp_keepalive: None
		}
	}

	pub fn host(&self) -> &str {
		self.host
	}

	pub fn port(&self) -> u16 {
		self.port
	}

	pub fn set_port(&mut self, port: u16) -> &mut Self {
		self.port = port;
		self
	}

	pub fn set_strategy(&mut self, strategy: IpStrategy) -> &mut Self {
		self.strategy = strategy;
		self
	}

	pub fn set_timeout(&mut self, timeout: Option<Duration>) -> &mut Self {
		self.timeout = timeout;
		self
	}

	pub fn set_recvbuf_size(&mut self, size: i32) -> &mut Self {
		self.recvbuf_size = Some(size);
		self
	}

	pub fn set_sendbuf_size(&mut self, size: i32) -> &mut Self {
		self.sendbuf_size = Some(size);
		self
	}

	pub fn set_tcp_nodelay(&mut self, enable: bool) -> &mut Self {
		self.tcp_nodelay = enable;
		self
	}

	pub fn set_tcp_keepalive(&mut self, idle: i32) -> &mut Self {
		self.tcp_keepalive = Some(idle);
		self
	}
}

pub struct Connection {
	inner: Socket
}

macro_rules! alias_func {
	($func: ident ($self: ident: $self_type: ty $(, $arg: ident: $type: ty)*) -> $return_type: ty) => {
		#[async_fn]
		pub async fn $func($self: $self_type $(, $arg: $type)*) -> $return_type {
			$self.inner.$func($($arg),*).await
		}
	}
}

impl Connection {
	alias_func!(shutdown(self: &Self, how: Shutdown) -> Result<()>);

	alias_func!(close(self: Self) -> Result<()>);

	alias_func!(recv(self: &Self, buf: &mut [u8], flags: u32) -> Result<usize>);

	alias_func!(send(self: &Self, buf: &[u8], flags: u32) -> Result<usize>);

	alias_func!(recv_vectored(self: &Self, bufs: &mut [IoSliceMut<'_>], flags: u32) -> Result<usize>);

	alias_func!(send_vectored(self: &Self, bufs: &[IoSlice<'_>], flags: u32) -> Result<usize>);

	#[async_fn]
	async fn connect_addrs<A: Iterator<Item = IpAddr>>(
		addrs: A, options: &ConnectOptions, stats: &mut ConnectStats
	) -> Result<Connection> {
		let mut error = None;
		let start = Instant::now();

		for ip in addrs {
			let addr = SocketAddr::new(ip, options.port).into();
			let socket =
				Socket::new_for_addr(&addr, SocketType::Stream as u32, IpProtocol::Tcp as u32)
					.await?;
			let connection = Connection { inner: socket };

			stats.tcp_tries += 1;

			debug!(target: &connection, "<< Connecting to {}:{} - Try {}: {}", options.host, options.port, stats.tcp_tries, ip);

			let now = Instant::now();

			match connection.inner.connect_addr(&addr).await {
				Ok(()) => {
					let elapsed = start.elapsed();

					stats.tcp_connect = elapsed;

					debug!(target: &connection, ">> Connected to {} ({:.3} ms elapsed, {:.3} ms total)", options.host, now.elapsed().as_secs_f32() * 1000.0, elapsed.as_secs_f32() * 1000.0);

					return Ok(connection);
				}

				Err(err) => {
					debug!(target: &connection, ">> Connection failed to {}: {} ({:.3} ms elapsed)", options.host, err.to_string(), now.elapsed().as_secs_f32() * 1000.0);

					connection.close().await?;
					error = Some(err);

					check_interrupt().await?;
				}
			}
		}

		Err(error.unwrap())
	}

	#[async_fn]
	async fn connect_to(
		options: &ConnectOptions, addrs: &LookupIp, stats: &mut ConnectStats
	) -> Result<Connection> {
		let v4 = addrs.v4().iter().map(|addr| IpAddr::V4(addr.clone()));
		let v6 = addrs.v6().iter().map(|addr| IpAddr::V6(addr.clone()));

		match options.strategy {
			IpStrategy::Default | IpStrategy::Ipv4Only => {
				Self::connect_addrs(v4, options, stats).await
			}

			IpStrategy::Ipv6Only => Self::connect_addrs(v6, options, stats).await,
			IpStrategy::PreferIpv4 => Self::connect_addrs(v4.chain(v6), options, stats).await,

			IpStrategy::PreferIpv6 => Self::connect_addrs(v6.chain(v4), options, stats).await
		}
	}

	#[async_fn]
	pub async fn connect_stats(options: &ConnectOptions) -> Result<(Connection, ConnectStats)> {
		let mut stats = ConnectStats::default();

		let addrs = {
			let now = Instant::now();
			let addrs = options.resolver.resolve_ips(options.host).await?;

			stats.dns_resolve = now.elapsed();
			addrs
		};

		let connection = match options.timeout {
			None => Self::connect_to(&options, &addrs, &mut stats).await?,
			Some(duration) => select(
				Self::connect_to(&options, &addrs, &mut stats),
				sleep(duration)
			)
			.await
			.first()
			.ok_or(Error::new(ErrorKind::TimedOut, "Connection timed out"))??
		};

		if let Some(size) = options.recvbuf_size {
			connection.inner.set_recvbuf_size(size).await?;
		}

		if let Some(size) = options.sendbuf_size {
			connection.inner.set_sendbuf_size(size).await?;
		}

		if options.tcp_nodelay {
			connection.inner.set_tcp_nodelay(true).await?;
		}

		if let Some(idle) = options.tcp_keepalive {
			connection.inner.set_tcp_keepalive(true, idle).await?;
		}

		Ok((connection, stats))
	}

	#[async_fn]
	pub async fn connect(options: &ConnectOptions) -> Result<Connection> {
		Ok(Self::connect_stats(options).await?.0)
	}

	pub fn has_peer_hungup(&mut self) -> Result<bool> {
		/* error and hangup are ignored in the input,
		 * we only use it to check for intersection
		 */
		let flags = make_bitflags!(PollFlag::{RdHangUp | HangUp | Error});

		/* sync polling because we don't care about waiting. async polling isn't any
		 * faster */
		let mut fds = [PollFd {
			fd: self.inner.fd().as_raw_fd(),
			events: flags.bits() as u16,
			returned_events: 0
		}];

		/* we shouldn't need to handle EINTR here because the timeout is 0 */
		if poll(&mut fds, 0)? == 0 {
			/* no events */
			Ok(false)
		} else {
			let returned_flags =
				unsafe { BitFlags::from_bits_unchecked(fds[0].returned_events as u32) };

			Ok(returned_flags.intersects(flags))
		}
	}

	#[async_fn]
	pub async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		let bits = ops::poll(self.inner.fd(), flags.bits()).await?;

		Ok(unsafe { BitFlags::from_bits_unchecked(bits) })
	}
}

#[async_trait_fn]
impl Read<Context> for Connection {
	async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
		self.recv(buf, 0).await
	}
}

#[async_trait_fn]
impl Write<Context> for Connection {
	async fn async_write(&mut self, buf: &[u8]) -> Result<usize> {
		self.send(buf, 0).await
	}
}

#[async_trait_fn]
impl Close<Context> for Connection {
	async fn async_close(self) -> Result<()> {
		self.close().await
	}
}
