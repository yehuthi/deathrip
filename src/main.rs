use std::{process, sync::Arc, time::SystemTime};

use clap::Parser;

const DEFAULT_EXTENSION: &str = "png";
const OUTPUT_HELP: &str = const_format::formatcp!(
	"Output file name with .png or .jp[e]g extension. Default: <Item ID>.{} or \
				{}_<unix-ms>.{} if the item ID cannot be determined.",
	DEFAULT_EXTENSION,
	env!("CARGO_PKG_NAME"),
	DEFAULT_EXTENSION
);

#[derive(clap::Parser)]
#[clap(author, version, about)]
struct Cli {
	/// URL to the image page, image base, or item ID.
	image: String,
	/// The zoom / resolution level. Must be >= 0. Leave unspecified for maximum.
	#[clap(short, long, parse(try_from_str=cli_validate_zoom))]
	zoom: Option<usize>,
	#[clap(help = OUTPUT_HELP, short, long)]
	output: Option<String>,
}

fn cli_validate_zoom(zoom: &str) -> Result<usize, String> {
	let zoom = zoom.parse::<isize>().map_err(|e| e.to_string())?;
	if zoom >= 0 {
		Ok(zoom as usize)
	} else {
		Err(String::from("Zoom level must be >= 0"))
	}
}

async fn cli() -> Result<(), Box<dyn std::error::Error>> {
	let cli = Cli::parse();
	let client = Arc::new(reqwest::Client::new());

	let (url, out) = {
		if let Ok(input) = deathrip::Input::try_from(cli.image.as_str()) {
			let normalized = match input {
				deathrip::Input::BaseUrl(url) => Ok((url, None)),
				deathrip::Input::PageUrl(url) => Err(url),
				deathrip::Input::ItemId(id) => Err(format!(
					"https://www.deadseascrolls.org.il/explore-the-archive/image/{}",
					id
				)),
			};
			match normalized {
				Ok(base) => base,
				Err(page_url) => {
					let page = deathrip::Page::try_fetch(&client, &page_url).await?;
					(page.base_url, Some(page.title))
				}
			}
		} else {
			eprintln!("Failed to determine the image type.");
			std::process::exit(1);
		}
	};

	let page = deathrip::Page {
		title: out.unwrap_or_else(|| {
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

	let zoom = if let Some(zoom) = cli.zoom {
		zoom
	} else {
		deathrip::determine_max_zoom(Arc::clone(&client), &page.base_url, 4).await?
	};
	deathrip::rip(client, &page.base_url, zoom, 8).await?.save(
		cli.output
			.unwrap_or_else(|| format!("{}.{}", page.title, DEFAULT_EXTENSION)),
	)?;
	Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	if let Err(e) = cli().await {
		eprintln!("Error: {}", e);
		process::exit(1);
	}
}
