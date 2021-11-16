fn main() {
	let app = clap::App::new(env!("CARGO_PKG_NAME"))
		.version(env!("CARGO_PKG_VERSION"))
		.author(env!("CARGO_PKG_AUTHORS"))
		.about(env!("CARGO_PKG_DESCRIPTION"))
		.arg(
			clap::Arg::with_name("URL")
				.required(true)
				.help("URL to the image base (temporary, will be URL to the image page)."),
		);
	let matches = app.get_matches();
	let url = matches.value_of("URL").unwrap();
	dbg!(url);
}
