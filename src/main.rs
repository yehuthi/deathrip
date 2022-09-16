use std::{process::ExitCode, sync::Arc, time::SystemTime};

use clap::Parser;
use tracing::metadata::LevelFilter;

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
    /// Level of verbosity. Specify multiple times for more verbosity (up to 4 times). Overridden
    /// by quiet.
    #[clap(short, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Suppress output (overrides verbose).
    #[clap(short, long)]
    quiet: bool,
}

fn cli_validate_zoom(zoom: &str) -> Result<usize, String> {
	let zoom = zoom.parse::<isize>().map_err(|e| e.to_string())?;
	if zoom >= 0 {
		Ok(zoom as usize)
	} else {
		Err(String::from("Zoom level must be >= 0"))
	}
}

impl<'a> From<&'a Cli> for LevelFilter {
    fn from(cli: &'a Cli) -> Self {
        if cli.quiet {
            LevelFilter::OFF
        } else {
            match cli.verbose {
                0 => LevelFilter::ERROR,
                1 => LevelFilter::WARN,
                2 => LevelFilter::INFO,
                3 => LevelFilter::DEBUG,
                _ => LevelFilter::TRACE,
            }
        }
    }
}

async fn cli() -> Result<(), Box<dyn std::error::Error>> {
	let cli = Cli::parse();
    
    let verbosity = LevelFilter::from(&cli);
    if verbosity != LevelFilter::OFF {
        tracing_subscriber::FmtSubscriber::builder()
            .without_time()
            .with_max_level(verbosity)
            .init();
    }

    let client = Arc::new(reqwest::Client::new());

    let (url, out) = {
        if let Ok(input) = deathrip::Input::try_from(cli.image.as_str()) {
            let normalized = match input {
                deathrip::Input::BaseUrl(url) => Ok((url, None)),
                deathrip::Input::PageUrl(url) => Err(url),
                deathrip::Input::ItemId(id) => Err(format!("https://www.deadseascrolls.org.il/explore-the-archive/image/{id}")),
            };
            match normalized {
                Ok(base) => base,
                Err(page_url) => {
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
        .unwrap_or_else(|| format!("{}.{DEFAULT_EXTENSION}", page.title)),
        )?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if let Err(e) = cli().await {
        tracing::error!("{e}");
        ExitCode::FAILURE
    } else { ExitCode::SUCCESS }
}
