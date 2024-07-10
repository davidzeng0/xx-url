use super::*;

#[derive(Clone)]
pub struct Answer {
	pub query: Query<'static>,
	pub records: Vec<Record<'static>>,
	pub valid_until: Option<Instant>
}

impl Answer {
	#[must_use]
	pub const fn new(
		query: Query<'static>, records: Vec<Record<'static>>, valid_until: Option<Instant>
	) -> Self {
		Self { query, records, valid_until }
	}
}

#[asynchronous(impl(ref, mut, box))]
pub trait Lookup {
	async fn lookup(&self, queries: &Query<'_>) -> Result<Answer>;
}
