//! What the statistics page shows. The numbers live in the database (`history`, see SPEC 6.4); this
//! is only their shape.

use serde::{Deserialize, Serialize};

use crate::Category;

/// How far back the statistics reach. Days are the user's days (the client sends its UTC offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum StatsRange {
    /// Since midnight.
    #[serde(rename = "today")]
    Today,
    /// The last 7 days, today included.
    #[serde(rename = "7d")]
    Week,
    /// The last 30 days, today included.
    #[serde(rename = "30d")]
    Month,
    /// Everything there is.
    #[default]
    #[serde(rename = "all")]
    All,
}

impl StatsRange {
    /// How many days, today included; `None` for everything.
    pub fn days(self) -> Option<u32> {
        match self {
            StatsRange::Today => Some(1),
            StatsRange::Week => Some(7),
            StatsRange::Month => Some(30),
            StatsRange::All => None,
        }
    }
}

/// The biggest file that was downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LargestFile {
    pub name: String,
    /// Bytes.
    pub size: u64,
}

/// Completed downloads of one category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryStat {
    pub category: Category,
    pub count: u64,
    /// Bytes.
    pub bytes: u64,
}

/// One day of the activity chart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DayStat {
    /// Unix time of the start of the day (in the client's time zone).
    pub start: i64,
    /// Downloads that ended that day, whatever way.
    pub count: u64,
    /// Bytes downloaded by them.
    pub bytes: u64,
}

/// A summary of the downloads of a period.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Stats {
    pub range: StatsRange,
    /// Downloads that finished successfully in the period.
    pub completed: u64,
    /// Downloads that failed in the period.
    pub failed: u64,
    /// Downloads that were cancelled (or removed while unfinished) in the period.
    pub cancelled: u64,
    /// Downloads that are not finished right now (queued, running, paused). Not tied to the period.
    pub in_progress: u64,
    /// Bytes fetched by the downloads of the period, plus what unfinished ones have fetched so far.
    pub bytes: u64,
    /// Total size of the completed files, bytes.
    pub completed_bytes: u64,
    /// Seconds spent actually transferring (paused and queued time is not counted).
    pub active_seconds: u64,
    /// Bytes per second over the time spent transferring; `0` when nothing was timed yet.
    pub average_speed: u64,
    /// The highest speed ever seen, bytes per second (all time, whatever the period).
    pub peak_speed: u64,
    pub largest: Option<LargestFile>,
    /// Completed downloads per category, biggest first.
    pub categories: Vec<CategoryStat>,
    /// The last 14 days, oldest first, empty days included (whatever the period).
    pub days: Vec<DayStat>,
}
