use std::{
	cell::OnceCell,
	sync::{Arc, Mutex},
	time::Instant
};

use rustls::ClientConfig;
use xx_core::debug;

use super::*;
use crate::{dns::Resolver, tls::certs::load_system_certs};

#[derive(Clone)]
struct GlobalData {
	dns_resolver: Arc<Resolver>,
	tls_client_config: Arc<ClientConfig>
}

#[derive(Clone)]
struct ThreadLocalData {
	dns_resolver: Arc<Resolver>,
	tls_client_config: Arc<ClientConfig>
}

static GLOBAL_DATA: Mutex<Option<GlobalData>> = Mutex::new(None);

thread_local! {
	static THREAD_LOCAL_DATA: OnceCell<ThreadLocalData> = const { OnceCell::new() };
}

#[asynchronous]
async fn create_global_data() -> GlobalData {
	let start = Instant::now();

	debug!("++ Initializing shared data");

	let config = {
		let certs = load_system_certs().await.expect("Failed to load certs");

		Arc::new(
			ClientConfig::builder()
				.with_root_certificates(certs)
				.with_no_client_auth()
		)
	};

	let resolver = Arc::new(
		Resolver::new()
			.await
			.expect("Failed to initialize DNS resolver")
	);

	debug!(
		"== Initialized shared data in {:.3} ms",
		start.elapsed().as_secs_f32() * 1000.0
	);

	GlobalData { dns_resolver: resolver, tls_client_config: config }
}

#[asynchronous]
async fn get_global_data() -> GlobalData {
	if let Some(config) = &*GLOBAL_DATA.lock().unwrap() {
		return config.clone();
	}

	let data = create_global_data().await;

	*GLOBAL_DATA.lock().unwrap() = Some(data.clone());

	data
}

#[asynchronous]
async fn create_thread_local_data() -> ThreadLocalData {
	let data = get_global_data().await;

	ThreadLocalData {
		dns_resolver: data.dns_resolver,
		tls_client_config: data.tls_client_config
	}
}

#[asynchronous]
async fn get_data() -> ThreadLocalData {
	let data = THREAD_LOCAL_DATA.with(|data| data.get().cloned());

	if let Some(data) = data {
		return data;
	}

	let data = create_thread_local_data().await;

	THREAD_LOCAL_DATA.with(|tls| {
		let _ = tls.set(data.clone());
	});

	data
}

#[allow(clippy::must_use_candidate, clippy::missing_const_for_fn)]
pub fn resolver_conf_path() -> &'static str {
	"/etc/resolv.conf"
}

#[allow(clippy::must_use_candidate, clippy::missing_const_for_fn)]
pub fn hosts_path() -> &'static str {
	"/etc/hosts"
}

#[allow(clippy::must_use_candidate, clippy::missing_const_for_fn)]
pub fn root_certs_path() -> &'static str {
	"/etc/ssl/certs"
}

#[asynchronous]
pub async fn get_tls_client_config() -> Arc<ClientConfig> {
	get_data().await.tls_client_config
}

#[asynchronous]
pub async fn get_resolver() -> Arc<Resolver> {
	get_data().await.dns_resolver
}

#[allow(clippy::missing_panics_doc)]
pub fn free_data() {
	GLOBAL_DATA.lock().unwrap().take();

	debug!("-- Uninitialized shared data");
}
