use std::{
	cell::RefCell,
	sync::{Arc, Mutex},
	time::Instant
};

use rustls::ClientConfig;
use xx_core::{debug, pointer::Ptr};
use xx_pulse::*;

use crate::{dns::Resolver, tls::certs::load_system_certs};

struct GlobalData {
	dns_resolver: Arc<Resolver>,
	tls_client_config: Arc<ClientConfig>
}

struct ThreadLocalData {
	dns_resolver: Arc<Resolver>,
	tls_client_config: Arc<ClientConfig>
}

static mut GLOBAL_DATA: Mutex<Option<GlobalData>> = Mutex::new(None);

thread_local! {
	static THREAD_LOCAL_DATA: RefCell<Option<ThreadLocalData>> = RefCell::new(None);
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

	let resolver = {
		let mut runtime = Runtime::new().expect("Failed to create runtime");

		Arc::new(
			runtime
				.block_on(Resolver::new())
				.expect("Failed to initialize DNS resolver")
		)
	};

	debug!(
		"== Initialized shared data in {:.3} ms",
		start.elapsed().as_secs_f32() * 1000.0
	);

	GlobalData { dns_resolver: resolver, tls_client_config: config }
}

fn get_global_data() -> &'static GlobalData {
	let mut data = unsafe { &GLOBAL_DATA }.lock().unwrap();

	if let Some(config) = &*data {
		return unsafe { Ptr::from(config).as_ref() };
	}

	*data = Some(create_global_data());

	unsafe { Ptr::from((&*data).as_ref().unwrap()).as_ref() }
}

fn create_thread_local_data() -> ThreadLocalData {
	let data = get_global_data();

	ThreadLocalData {
		dns_resolver: data.dns_resolver.clone(),
		tls_client_config: data.tls_client_config.clone()
	}
}

fn get_data() -> &'static ThreadLocalData {
	unsafe {
		THREAD_LOCAL_DATA
			.with(|data| {
				if let Some(data) = &*data.borrow() {
					return Ptr::from(data);
				}

				*data.borrow_mut() = Some(create_thread_local_data());

				data.borrow().as_ref().unwrap().into()
			})
			.as_ref()
	}
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
	get_data().tls_client_config.clone()
}

pub fn get_resolver() -> Arc<Resolver> {
	get_data().dns_resolver.clone()
}

pub fn free_data() {
	unsafe { &GLOBAL_DATA }.lock().unwrap().take();

	debug!("-- Uninitialized shared data");
}
