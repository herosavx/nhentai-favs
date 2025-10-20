use crate::error::{AppError, Result};
use argh::FromArgs;
use std::ops::RangeInclusive;

#[derive(FromArgs, Debug)]
/// Command-line tool for exporting nhentai favorite list
pub struct Args {
    /// whether to download cover images (default: false)
    #[argh(switch, short = 't')]
    pub thumbnail: bool,

    /// page range to fetch data from (e.g. 1-20, 20-1). Data fetched from higher to lower page
    #[argh(option, short = 'p', from_str_fn(parse_page_range))]
    pub page_range: Option<RangeInclusive<u32>>,

    /// convert existing database to csv format in outpath. No network operation performed
    #[argh(switch, short = 'c')]
    pub export_csv: bool,

    /// restore previous state of database if corrupted by previous run
    #[argh(switch)]
    pub restore: bool,

    /// output directory, may contain existing database and config.json
    #[argh(option, short = 'o')]
    pub outpath: String,
}

impl Args {
    pub fn validate_page_range(&self, max_pages: u32) -> Result<()> {
        if let Some(range) = &self.page_range {
            if *range.end() > max_pages {
                return Err(AppError::Validation(format!(
                    "End page {} exceeds total pages {}",
                    range.end(),
                    max_pages
                )));
            }
        }
        Ok(())
    }

    pub fn get_page_range(&self, default_max: u32) -> RangeInclusive<u32> {
        self.page_range.clone().unwrap_or(1..=default_max)
    }
}

fn parse_page_range(value: &str) -> std::result::Result<RangeInclusive<u32>, String> {
    let parts: Vec<&str> = value.split('-').collect();

    let first: u32 = parts
        .first()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| "Invalid range format. Expected: page1-page2 (e.g., 1-20)".to_string())?;

    let second: u32 = parts
        .get(1)
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| "Invalid range format. Expected: page1-page2 (e.g., 1-20)".to_string())?;

    if first < 1 || second < 1 {
        return Err("Page numbers must be greater than 0".to_string());
    }

    let (start, end) = if first >= second {
        (second, first)
    } else {
        (first, second)
    };

    Ok(start..=end)
}
