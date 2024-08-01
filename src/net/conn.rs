use std::io::{IoSlice, IoSliceMut};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xx_core::async_std::io::*;
use xx_core::debug;
use xx_core::enumflags2::{make_bitflags, BitFlags};
use xx_core::macros::wrapper_functions;
use xx_core::os::epoll::PollFlag;
use xx_core::os::inet::IpProtocol;
use xx_core::os::poll::{self, poll, BorrowedPollFd};
use xx_core::os::socket::{MessageFlag, Shutdown, SocketType};
use xx_pulse::impls::TaskExt;
use xx_pulse::net::*;

use super::*;
use crate::dns::{LookupIp, Resolver};

#[derive(Default, Clone, Copy)]
pub struct ConnectStats {
	pub dns_resolve: Duration,
	pub tcp_tries: u32,
	pub tcp_connect: Duration
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IpStrategy {
	#[default]
	Default,
	Ipv4Only,
	Ipv6Only,
	PreferIpv4,
	PreferIpv6
}

pub struct ConnectOptions<'host> {
	resolver: Arc<Resolver>,
	host: &'host str,
	port: u16,
	strategy: IpStrategy,
	timeout: Option<Duration>,
	recvbuf_size: Option<i32>,
	sendbuf_size: Option<i32>,
	tcp_nodelay: bool,
	tcp_keepalive: Option<i32>
}

impl<'host> ConnectOptions<'host> {
	#[asynchronous]
	pub async fn new(host: &'host str, port: u16) -> Self {
		Self::with_resolver(get_resolver().await, host, port)
	}

	#[must_use]
	pub const fn with_resolver(resolver: Arc<Resolver>, host: &'host str, port: u16) -> Self {
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

	#[must_use]
	pub const fn host(&self) -> &'host str {
		self.host
	}

	#[must_use]
	pub const fn port(&self) -> u16 {
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

pub struct Conn {
	inner: Socket
}

#[asynchronous]
impl Conn {
	wrapper_functions! {
		inner = self.inner;

		#[asynchronous]
		pub async fn recv(&mut self, buf: &mut [u8], flags: BitFlags<MessageFlag>) -> Result<usize>;

		#[asynchronous]
		pub async fn send(&mut self, buf: &[u8], flags: BitFlags<MessageFlag>) -> Result<usize>;

		#[asynchronous]
		pub async fn recv_vectored(&mut self, bufs: &mut [IoSliceMut<'_>], flags: BitFlags<MessageFlag>) -> Result<usize>;

		#[asynchronous]
		pub async fn send_vectored(&mut self, bufs: &[IoSlice<'_>], flags: BitFlags<MessageFlag>) -> Result<usize>;

		#[asynchronous]
		pub async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

		#[asynchronous]
		pub async fn shutdown(&mut self, how: Shutdown) -> Result<()>;

		#[asynchronous]
		pub async fn close(self) -> Result<()>;
	}

	async fn connect_addrs<A>(
		addrs: A, options: &ConnectOptions<'_>, stats: &mut ConnectStats
	) -> Result<Self>
	where
		A: Iterator<Item = IpAddr>
	{
		let mut error = None;
		let start = Instant::now();

		for ip in addrs {
			let addr = SocketAddr::new(ip, options.port).into();
			let socket =
				Socket::new_for_addr(&addr, SocketType::Stream as u32, IpProtocol::Tcp).await?;
			let connection = Self { inner: socket };

			#[allow(clippy::arithmetic_side_effects)]
			(stats.tcp_tries += 1);

			debug!(target: &connection, "<< Connecting to {}:{} - Try {}: {}", options.host, options.port, stats.tcp_tries, ip);

			let now = Instant::now();

			match connection.inner.connect(&addr).await {
				Ok(()) => {
					let elapsed = start.elapsed();

					stats.tcp_connect = elapsed;

					debug!(target: &connection, ">> Connected to {} ({:.3} ms elapsed, {:.3} ms total)", options.host, now.elapsed().as_secs_f32() * 1000.0, elapsed.as_secs_f32() * 1000.0);

					return Ok(connection);
				}

				Err(err) => {
					debug!(target: &connection, ">> Connection failed to {}: {} ({:.3} ms elapsed)", options.host, err.to_string(), now.elapsed().as_secs_f32() * 1000.0);

					error = Some(err);

					check_interrupt().await?;
				}
			}
		}

		Err(error.unwrap_or_else(|| common::NO_ADDRESSES.into()))
	}

	async fn connect_to(
		options: &ConnectOptions<'_>, addrs: &LookupIp, stats: &mut ConnectStats
	) -> Result<Self> {
		let v4 = addrs.v4().iter().map(|addr| IpAddr::V4(*addr));
		let v6 = addrs.v6().iter().map(|addr| IpAddr::V6(*addr));

		match options.strategy {
			IpStrategy::PreferIpv4 => Self::connect_addrs(v4.chain(v6), options, stats).await,
			IpStrategy::Ipv4Only => Self::connect_addrs(v4, options, stats).await,
			IpStrategy::Ipv6Only => Self::connect_addrs(v6, options, stats).await,
			IpStrategy::Default | IpStrategy::PreferIpv6 => {
				Self::connect_addrs(v6.chain(v4), options, stats).await
			}
		}
	}

	pub async fn connect_stats(options: &ConnectOptions<'_>) -> Result<(Self, ConnectStats)> {
		let mut stats = ConnectStats::default();

		let addrs = {
			let now = Instant::now();
			let addrs = options.resolver.resolve_ips(options.host).await?;

			stats.dns_resolve = now.elapsed();
			addrs
		};

		let connection = match options.timeout {
			None => Self::connect_to(options, &addrs, &mut stats).await?,
			Some(duration) => Self::connect_to(options, &addrs, &mut stats)
				.timeout(duration)
				.await
				.ok_or(common::CONNECT_TIMEOUT)??
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

	pub async fn connect(options: &ConnectOptions<'_>) -> Result<Self> {
		Ok(Self::connect_stats(options).await?.0)
	}

	pub fn has_peer_hungup(&self) -> Result<bool> {
		use poll::PollFlag;

		/* error and hangup are ignored by the syscall,
		 * we only use it to check for intersection
		 */
		let flags = make_bitflags!(PollFlag::{ RdHangUp | HangUp | Error });

		/* sync polling because we don't care about waiting, and async polling isn't
		 * any faster */
		let mut fds = [BorrowedPollFd::new(self.inner.fd(), flags)];

		/* we shouldn't need to handle EINTR here because the timeout is 0 */
		if poll(&mut fds, Duration::ZERO)? == 0 {
			/* no events */
			Ok(false)
		} else {
			Ok(fds[0].returned_events().intersects(flags))
		}
	}
}

impl Read for Conn {
	read_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

impl Write for Conn {
	write_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

impl SplitMut for Conn {
	type Reader<'a> = SocketHalf<'a>;
	type Writer<'a> = SocketHalf<'a>;

	fn try_split(&mut self) -> Result<(Self::Reader<'_>, Self::Writer<'_>)> {
		self.inner.try_split()
	}
}
