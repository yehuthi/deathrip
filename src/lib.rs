mod util;

use std::{
	convert::Infallible,
	fmt::{self, Display},
	io::Cursor,
	sync::{
		atomic::{self, AtomicUsize},
		Arc,
	},
};

use image::{GenericImage, GenericImageView};
use itertools::Itertools as _;
use reqwest::Client;
use tokio::sync::{Mutex, RwLock};
use util::StringMutTail;

/// Input to the main operation, i.e. reference to the desired image.
#[derive(Debug, Hash, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub enum Input {
	/// The base URL of the image.
	///
	/// This variant is a bit niche for end-users: it is the URL one would get by going to an item
	/// page, right-clicking on the image, copying the image link, and removing the `=` and the
	/// parameters after it.
	///
	/// For example, `https://lh5.ggpht.com/IFfrGztWa5KuIWKn2qAwASLds6reQ5IR8l8ColqH6I81oHWBITZ2I9ET`.
	BaseUrl(String),
	/// The URL for the image's page.
	///
	/// For example, `https://www.deadseascrolls.org.il/explore-the-archive/image/B-497904`.
	PageUrl(String),
	/// The item ID of the image.
	///
	/// E.g. for [this item](https://www.deadseascrolls.org.il/explore-the-archive/image/B-497904)
	/// it is B-497904, which is specified in the page itself (top of left pane), and at the end of
	/// the URL.
	ItemId(String),
}

impl AsRef<str> for Input {
	fn as_ref(&self) -> &str {
		match self {
			Input::BaseUrl(s) => s.as_str(),
			Input::PageUrl(s) => s.as_str(),
			Input::ItemId(s) => s.as_str(),
		}
	}
}

impl Display for Input {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(self.as_ref()) }
}

/// Attempts to infer the type of input.
///
/// Currently always succeeds with [`Input::ItemId`](Input::ItemId) as fallback, but may change later.
impl TryFrom<&str> for Input {
	type Error = Infallible;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		let value = value.to_owned();
		Ok(if value.contains("ggpht.com") {
			Self::BaseUrl(value)
		} else if value.contains("deadseascrolls.org") {
			Self::PageUrl(value)
		} else {
			Self::ItemId(value)
		})
	}
}

/// Determines the limit of an axis for the image.
///
/// - The `base` parameter is the base URL of the image along with `=` and XYZ parameters (see section below), but with the
/// target axis parameter last and without a value (e.g. end with `=x0-y0-z` to target the Z axis).
/// - The `num_workers` is the amount of simultaneous requests that will be made.
///
/// ## Base URL
///
/// The base URL for this function is not the same as the base for [`rip`](rip).
/// This one requires partial parameterization.
///
/// The image base URL is appended with `=` and X, Y, and Z values in the format:
/// `=x<X>-y<Y>-z<Z>`. The order of the axes is insignificant.
/// X and Y refer to position and Z refers to the resolution.
///
/// This function will send HEAD requests, incrementing an axis determined by the base URL,
/// and will return the highest value that succeeds.
async fn determine_limit(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	// A variable dedicated for the result.
	// It's a `Result` that will be the minimal value that succeeds or an error if we encounter an
	// error (that isn't a client-error because we took the axis too far).
	let min_failure = Arc::new(RwLock::new(Ok::<usize, reqwest::Error>(usize::MAX)));
	// An atomic counter of the axis value. Threads read and increment it as they try higher axis values.
	let i = Arc::new(AtomicUsize::new(1));

	let workers = (0..num_workers).map(|_| {
		let mut base = StringMutTail::from(base);
		let i = Arc::clone(&i);
		let min_failure = Arc::clone(&min_failure);
		let client = client.clone();
		tokio::spawn(async move {
			loop {
				let client = client.as_ref();
				let level = i.fetch_add(1, atomic::Ordering::SeqCst);
				let response = client
					.head(base.with_tail_int(level))
					.send()
					.await
					.and_then(|r| r.error_for_status());
				match response {
					Ok(_) => {}
					Err(e) if e.status().map_or(false, |c| c.is_client_error()) => {
						let mut current_result = min_failure.write().await;
						match *current_result {
							Ok(previous_level) if level <= previous_level => {
								*current_result = Ok(level);
							}
							Ok(_) => {}
							Err(_) => {}
						}
						break;
					}
					Err(e) => {
						*min_failure.write().await = Err(e);
						break;
					}
				}
			}
		})
	});

	futures::future::try_join_all(workers).await.unwrap();
	Arc::try_unwrap(min_failure)
		.unwrap()
		.into_inner()
		.map(|l| l - 1)
}

/// Determines the max zoom level for the image at the base URL.
pub async fn determine_max_zoom(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	determine_limit(client, &format!("{}=x0-y0-z", base), num_workers).await
}

/// Determines the count of columns i.e. the amount of cells going across the image.
pub async fn determine_columns(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	zoom: usize,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	let base = format!("{}=z{}-y0-x", base, zoom);
	determine_limit(client, &base, num_workers)
		.await
		.map(|c| c + 1)
}

/// Determines the count of rows i.e. the amount of cells going along the image.
pub async fn determine_rows(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	zoom: usize,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	let base = format!("{}=z{}-x0-y", base, zoom);
	determine_limit(client, &base, num_workers)
		.await
		.map(|c| c + 1)
}

/// Determines the [rows](determine_rows) and [columns](determine_columns) of the image (in-parallel).
pub async fn determine_dimensions(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	zoom: usize,
	num_workers_half: usize,
) -> Result<(usize, usize), reqwest::Error> {
	tokio::try_join!(
		determine_columns(Clone::clone(&client), base, zoom, num_workers_half),
		determine_rows(client, base, zoom, num_workers_half)
	)
}

/// An error when fetching or processing an image.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// Failure trying to fetch an image or metadata.
	#[error("HTTP error: {0}")]
	HttpError(#[from] reqwest::Error),
	/// Failure trying to decode an image.
	#[error("image processing error: {0}")]
	ImageError(#[from] image::ImageError),
	/// Failure trying to determine the image's format.
	#[error("image format inference error: {0}")]
	ImageFormatGuessError(std::io::Error),
}

/// Rips an image from the given base URL.
///
/// `num_workers_half` corresponds to half of the amount of parallel connections that will be used to
/// fetch metadata (half because at most two operations will get this limit in parallel).
pub async fn rip(
	client: impl AsRef<Client> + 'static + Send + Clone,
	base: &str,
	zoom: usize,
	num_workers_half: usize,
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Error> {
	let dims_task = {
		let client = Clone::clone(&client);
		async {
			determine_dimensions(client, base, zoom, num_workers_half)
				.await
				.map_err(Error::HttpError)
		}
	};
	let fetch_cell_client = Clone::clone(&client);
	let fetch_cell = |(x, y): (usize, usize)| {
		tracing::trace!("fetching cell ({x},{y})");
		let client = Clone::clone(&fetch_cell_client);
		async move {
			let data = client
				.as_ref()
				.get(format!("{}=x{}-y{}-z{}", base, x, y, zoom))
				.send()
				.await?
				.error_for_status()?
				.bytes()
				.await?;
			image::io::Reader::new(Cursor::new(data))
				.with_guessed_format()
				.map_err(Error::ImageFormatGuessError)?
				.decode()
				.map_err(Error::ImageError)
		}
	};
	let head_task = fetch_cell((0, 0));
	let ((columns, rows), head) = tokio::try_join!(dims_task, head_task)?;
	tracing::trace!("determined {columns} columns \u{00D7} {rows} rows");
	let (tile_width, tile_height) = head.dimensions();
	let image_width = columns as u32 * tile_width;
	let image_height = rows as u32 * tile_height;
	tracing::trace!("cell size is {tile_width}\u{00D7}{tile_height}, total image size will be {image_width}\u{00D7}{image_height}");

	let mut image = image::ImageBuffer::new(image_width, image_height);
	image.copy_from(&head, 0, 0)?;

	let image = Arc::new(Mutex::new(image));
	let cells = (0..columns).cartesian_product(0..rows).skip(1);
	futures::future::try_join_all(cells.map(|(x, y)| {
		let image = Arc::clone(&image);
		async move {
			let cell = fetch_cell((x, y)).await?;
			tracing::trace!("fetched cell ({x},{y})");
			image
				.lock()
				.await
				.copy_from(&cell, x as u32 * tile_width, y as u32 * tile_height)?;
			Ok::<(), Error>(())
		}
	}))
	.await
	.unwrap();

	Ok(Arc::try_unwrap(image).unwrap().into_inner())
}

#[derive(Debug, thiserror::Error)]
pub enum PageError {
	#[error("HTTP error fetching page metadata: {0}")]
	HttpError(#[from] reqwest::Error),
	#[error("failed to find the base image URL in the page")]
	BaseNotFound,
	#[error("failed to find the page title in the page")]
	TitleNotFound,
}

#[derive(Debug, Hash, Default, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub struct Page {
	pub title:    String,
	pub base_url: String,
}

impl Page {
	pub async fn try_fetch(client: &Client, page_url: &str) -> Result<Self, PageError> {
		let response = client.get(page_url).send().await?.text().await?;
		let base_url = {
			let regex =
				regex::Regex::new(r#"<image-viewer[\s\S]+?url="(?P<url>https[^"]+)"#).unwrap();
			regex
				.captures(&response)
				.and_then(|captures| captures.name("url"))
				.ok_or(PageError::BaseNotFound)?
				.as_str()
				.to_owned()
		};

		let title = {
			let regex =
				regex::Regex::new(r"<title>\s*[^-]+-\s*(?P<title>[^<]+?)\s*</title>").unwrap();
			regex
				.captures(&response)
				.and_then(|captures| captures.name("title"))
				.ok_or(PageError::TitleNotFound)?
				.as_str()
				.to_owned()
		};

		Ok(Self { title, base_url })
	}
}
