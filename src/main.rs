use std::{
	io::{Cursor, IsTerminal, Write},
	path::PathBuf,
	process::ExitCode,
	sync::Arc,
	time::{Instant, SystemTime},
};

use clap::Parser;
use image::ImageOutputFormat;
use tokio::fs;
use tracing::{metadata::LevelFilter, Instrument};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

const DEFAULT_EXTENSION: &str = "png";
const OUTPUT_HELP: &str = const_format::formatcp!(
	"Output file name. Default: <Item ID>.{} or {}_<unix-ms>.{} if the item ID cannot be determined.",
	DEFAULT_EXTENSION,
	env!("CARGO_PKG_NAME"),
	DEFAULT_EXTENSION
);

#[derive(clap::Parser)]
#[clap(author, version, about)]
struct Cli {
	/// URL to the image page, image base, or item ID.
	image:   String,
	/// The zoom / resolution level. Must be >= 0. Leave unspecified for maximum.
	#[clap(short, long, value_parser = cli_validate_zoom)]
	zoom:    Option<usize>,
	/// The output file. If missing, it will be auto-generated, unless the output is piped.
	#[clap(help = OUTPUT_HELP, short, long)]
	output:  Option<PathBuf>,
	/// The output format. Possible options are: png | jp[e]g[<Q>] | bmp | gif | tiff | tga | ico | [open]exr | farbfeld.
	/// The variable Q is a number within [0,100] that controls quality (higher is better).
	#[clap(short, long, default_value = "png", value_parser = parse_format)]
	format:  ImageOutputFormat,
	/// Verbose output. Overridden by quiet.
	#[clap(short, long)]
	verbose: bool,
	/// Suppress output. Overrides verbose.
	#[clap(short, long)]
	quiet:   bool,
}

fn parse_format(format: &str) -> Result<ImageOutputFormat, &'static str> {
	let format = format.to_ascii_lowercase();
	match format.as_str() {
		"png" => Ok(ImageOutputFormat::Png),
		"bmp" => Ok(ImageOutputFormat::Bmp),
		"gif" => Ok(ImageOutputFormat::Gif),
		"ico" => Ok(ImageOutputFormat::Ico),
		"farbfeld" => Ok(ImageOutputFormat::Farbfeld),
		"tga" => Ok(ImageOutputFormat::Tga),
		"exr" | "openexr" => Ok(ImageOutputFormat::OpenExr),
		"tiff" => Ok(ImageOutputFormat::Tiff),
		_ => {
			let jpg_len = if format.starts_with("jpg") {
				Some(3)
			} else if format.starts_with("jpeg") {
				Some(4)
			} else {
				None
			};
			if let Some(jpg_len) = jpg_len {
				let quality = &format[jpg_len..];
				if quality.is_empty() {
					Ok(ImageOutputFormat::Jpeg(100))
				} else if let Ok(quality) = quality.parse::<u8>() {
					Ok(ImageOutputFormat::Jpeg(quality.min(100)))
				} else {
					Err("couldn't parse the quality, it should be a number within [0,100]")
				}
			} else {
				Err("unrecognized image output format")
			}
		}
	}
}

fn cli_validate_zoom(zoom: &str) -> Result<usize, &'static str> {
	let zoom = zoom
		.parse::<isize>()
		.map_err(|_| "zoom should be a number >= 0")?;
	if zoom >= 0 {
		Ok(zoom as usize)
	} else {
		Err("Zoom level must be >= 0")
	}
}

impl<'a> From<&'a Cli> for LevelFilter {
	fn from(cli: &'a Cli) -> Self {
		match (cli.quiet, cli.verbose) {
			(true, _) => LevelFilter::OFF,
			(false, false) => LevelFilter::INFO,
			(false, true) => LevelFilter::TRACE,
		}
	}
}

async fn cli() -> Result<(), Box<dyn std::error::Error>> {
	let cli = Cli::parse();

	let verbosity = LevelFilter::from(&cli);
	if verbosity != LevelFilter::OFF {
		tracing_subscriber::registry()
			.with(
				tracing_subscriber::fmt::layer()
					.with_writer(std::io::stderr)
					.without_time(),
			)
			.with(
				tracing_subscriber::filter::Targets::new()
					.with_target(env!("CARGO_PKG_NAME"), verbosity),
			)
			.init();
	}

	let time_start = Instant::now();

	let client = Arc::new(reqwest::Client::new());

	tracing::info!("determining metadata");
	let (url, out) = {
		if let Ok(input) = deathrip::Input::try_from(cli.image.as_str()) {
			let normalized = match input {
				deathrip::Input::BaseUrl(url) => Ok((url, None)),
				deathrip::Input::PageUrl(url) => Err(url),
				deathrip::Input::ItemId(id) => Err(format!(
					"https://www.deadseascrolls.org.il/explore-the-archive/image/{id}"
				)),
			};
			match normalized {
				Ok(base) => base,
				Err(page_url) => {
					tracing::info!("fetching metadata from page URL");
					let page = deathrip::Page::try_fetch(&client, &page_url).await?;
					(page.base_url, Some(page.title))
				}
			}
		} else {
			tracing::error!("failed to determine the image type.");
			std::process::exit(1);
		}
	};

	let page = deathrip::Page {
		title:    out.unwrap_or_else(|| {
			format!(
				"{}_{}",
				env!("CARGO_PKG_NAME"),
				SystemTime::now()
					.duration_since(SystemTime::UNIX_EPOCH)
					.map(|time| time.as_millis())
					.unwrap_or(0)
			)
		}),
		base_url: url,
	};

	let span_zoom = tracing::info_span!("determining zoom level").entered();
	let zoom = if let Some(zoom) = cli.zoom {
		tracing::trace!("user supplied zoom level {zoom}");
		zoom
	} else {
		let zoom = deathrip::determine_max_zoom(Arc::clone(&client), &page.base_url, 4).await?;
		tracing::info!("determined zoom level of {zoom}");
		zoom
	};
	drop(span_zoom);

	let image = deathrip::rip(client, &page.base_url, zoom, 8)
		.instrument(tracing::info_span!("ripping image"))
		.await?;
	let dur_rip = time_start.elapsed();
	tracing::info!("finished ripping image in {}ms", dur_rip.as_millis());

	let atty = std::io::stdout().is_terminal();
	if atty {
		let out_path = cli
			.output
			.unwrap_or_else(|| PathBuf::from(format!("{}.{DEFAULT_EXTENSION}", page.title)));
		tracing::info!("writing ripped image to output file {}", out_path.display());
		if let Some(parent) = out_path.parent() {
			fs::create_dir_all(parent).await?;
		}
		let mut out_file = fs::File::create(out_path).await?.into_std().await;
		image.write_to(&mut out_file, cli.format)?;
	} else {
		tracing::info!("writing ripped image to output stream");
		let (w, h) = image.dimensions();
		let mut buf = Vec::with_capacity(w as usize * h as usize * 3);
		image.write_to(&mut Cursor::new(&mut buf), cli.format)?;
		std::io::stdout().write_all(&buf)?;
	}

	let dur_total = time_start.elapsed();
	tracing::info!("finished in {}ms", dur_total.as_millis());
	Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
	if let Err(e) = cli().await {
		tracing::error!("{e}");
		ExitCode::FAILURE
	} else {
		ExitCode::SUCCESS
	}
}
