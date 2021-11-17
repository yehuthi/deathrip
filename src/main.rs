use std::{borrow::Cow, process, sync::Arc, time::SystemTime};

async fn cli() -> Result<(), Box<dyn std::error::Error>> {
	static DEFAULT_EXTENSION: &str = "png";
	let output_help = format!(
		"Output file name with .png or .jp[e]g extension. Default: <Item ID>.{} or \
				{}_<unix-ms>.{} if the item ID cannot be determined.",
		DEFAULT_EXTENSION,
		env!("CARGO_PKG_NAME"),
		DEFAULT_EXTENSION
	);
	let app = clap::App::new(env!("CARGO_PKG_NAME"))
		.version(env!("CARGO_PKG_VERSION"))
		.author(env!("CARGO_PKG_AUTHORS"))
		.about(env!("CARGO_PKG_DESCRIPTION"))
		.arg(
			clap::Arg::with_name("IMAGE")
				.required(true)
				.help("URL to the image page, image base, or item ID."),
		)
		.arg(
			clap::Arg::with_name("ZOOM")
				.short("z")
				.long("zoom")
				.takes_value(true)
				.validator(|z| {
					if let Ok(z) = z.parse::<usize>() {
						if z > 0 {
							return Ok(());
						}
					}
					Err("ZOOM must be a positive integer.".to_owned())
				})
				.help("The zoom / resolution level. Must be >= 0. Leave unspecified for maximum."),
		)
		.arg(
			clap::Arg::with_name("OUTPUT")
				.short("o")
				.long("output")
				.takes_value(true)
				.help(&output_help)
				.validator(|path| {
					let path = path.to_lowercase();
					if path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".jpeg") {
						Ok(())
					} else {
						Err("Output file must end with .png, .jpg or .jpeg.".into())
					}
				}),
		);
	let matches = app.get_matches();

	let client = Arc::new(reqwest::Client::new());

	let (url, out) = {
		if let Ok(input) = deathrip::Input::try_from(matches.value_of("IMAGE").unwrap()) {
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

	let zoom = if let Some(zoom) = matches.value_of("ZOOM") {
		zoom.parse::<usize>().unwrap()
	} else {
		deathrip::determine_max_zoom(Arc::clone(&client), &page.base_url, 4).await?
	};
	deathrip::rip(client, &page.base_url, zoom, 8).await?.save(
		matches
			.value_of("OUTPUT")
			.map_or_else(
				|| Cow::Owned(format!("{}.{}", page.title, DEFAULT_EXTENSION)),
				|out| Cow::Borrowed(out),
			)
			.as_ref(),
	)?;
	Ok(())
}

#[tokio::main]
async fn main() {
	if let Err(e) = cli().await {
		eprintln!("Error: {}", e);
		process::exit(1);
	}
}
