use std::{
	cell::RefCell,
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

static mut GLOBAL_DATA: Mutex<Option<GlobalData>> = Mutex::new(None);

thread_local! {
	static THREAD_LOCAL_DATA: RefCell<Option<ThreadLocalData>> = RefCell::new(None);
}

#[main]
async fn create_resolver() -> Result<Resolver> {
	Resolver::new().await
}

fn create_global_data() -> GlobalData {
	let start = Instant::now();

	debug!("++ Initializing shared data");

	let config = {
		let certs = load_system_certs().expect("Failed to load certs");

		Arc::new(
			ClientConfig::builder()
				.with_safe_defaults()
				.with_root_certificates(certs)
				.with_no_client_auth()
		)
	};

	let resolver = Arc::new(create_resolver().expect("Failed to initialize DNS resolver"));

	debug!(
		"== Initialized shared data in {:.3} ms",
		start.elapsed().as_secs_f32() * 1000.0
	);

	GlobalData { dns_resolver: resolver, tls_client_config: config }
}

fn get_global_data() -> GlobalData {
	let mut data = unsafe { &GLOBAL_DATA }.lock().unwrap();

	if let Some(config) = &*data {
		return config.clone();
	}

	data.insert(create_global_data()).clone()
}

fn create_thread_local_data() -> ThreadLocalData {
	let data = get_global_data();

	ThreadLocalData {
		dns_resolver: data.dns_resolver.clone(),
		tls_client_config: data.tls_client_config.clone()
	}
}

fn get_data() -> ThreadLocalData {
	THREAD_LOCAL_DATA.with(|data| {
		if let Some(data) = &*data.borrow() {
			return data.clone();
		}

		data.borrow_mut().insert(create_thread_local_data()).clone()
	})
}

pub fn resolver_conf_path() -> &'static str {
	"/etc/resolv.conf"
}

pub fn hosts_path() -> &'static str {
	"/etc/hosts"
}

pub fn root_certs_path() -> &'static str {
	"/etc/ssl/certs"
}

pub fn get_tls_client_config() -> Arc<ClientConfig> {
	get_data().tls_client_config
}

pub fn get_resolver() -> Arc<Resolver> {
	get_data().dns_resolver
}

pub fn free_data() {
	unsafe { &GLOBAL_DATA }.lock().unwrap().take();

	THREAD_LOCAL_DATA.take();

	debug!("-- Uninitialized shared data");
}
