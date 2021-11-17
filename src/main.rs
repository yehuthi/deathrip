use std::{borrow::Cow, sync::Arc, time::SystemTime};

#[tokio::main]
async fn main() {
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
			clap::Arg::with_name("OUTPUT")
				.short("o")
				.long("output")
				.takes_value(true)
				.help("Output file name with .png or .jp[e]g extension. Default: <Item ID>.png")
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
	let start = SystemTime::now();

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
					let page = deathrip::Page::try_fetch(&client, &page_url).await.unwrap();
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
					.unwrap()
					.as_millis()
			)
		}),
		base_url: url,
	};
	deathrip::rip(client, &page.base_url, 8)
		.await
		.unwrap()
		.save(
			matches
				.value_of("OUTPUT")
				.map_or_else(
					|| Cow::Owned(format!("{}.png", page.title)),
					|out| Cow::Borrowed(out),
				)
				.as_ref(),
		)
		.unwrap();
	println!("Elapsed {}ms", start.elapsed().unwrap().as_millis());
}
