//! Internal utilities.

/// A [`String`] buffer with a mutating tail.
#[derive(Debug, Hash, Default, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub struct StringMutTail {
	/// The [`String`] value.
	url:        String,
	/// The index of the tail.
	///
	/// The tail is at `&url[tail_index..]`.
	tail_index: usize,
}

impl From<String> for StringMutTail {
	fn from(mut base: String) -> Self {
		let tail_index = base.len();
		base.reserve(10);
		Self {
			url: base,
			tail_index,
		}
	}
}

impl From<&str> for StringMutTail {
	fn from(base: &str) -> Self { Self::from(base.to_string()) }
}

impl StringMutTail {
	/// Sets the [tail](StringMutTail::tail_index) to the given integer.
	pub fn with_tail_int(&mut self, integer: impl itoa::Integer) -> &str {
		self.url.truncate(self.tail_index);
		self.url.push_str(itoa::Buffer::new().format(integer));
		&self.url
	}
}
