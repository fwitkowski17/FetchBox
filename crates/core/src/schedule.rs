//! Download hours (SPEC 6.3.2): the pure decisions, without a clock.
//!
//! A *window* is a daily range of local clock minutes (`00:00`-`06:00`, or `22:00`-`06:00` across
//! midnight). While it is closed, downloads wait; a download can opt out (`start_now`) or ask to
//! begin at a set time (`start_at`). Turning the local time of a moment into a minute of the day,
//! and a minute of the day into the next moment, is the server's job (it needs the time zone).

/// Minutes in a day.
pub const DAY_MINUTES: u16 = 24 * 60;

/// A daily window in local minutes after midnight. `from` is inclusive, `to` exclusive; when
/// `to <= from` the window runs over midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub from: u16,
    pub to: u16,
}

impl Window {
    /// Both ends must be a minute of the day, and they must differ (equal ends would mean "all
    /// day", which is "no window").
    pub fn is_valid(&self) -> bool {
        self.from < DAY_MINUTES && self.to < DAY_MINUTES && self.from != self.to
    }

    /// Is the local minute of the day `minute` inside the window?
    pub fn contains(&self, minute: u16) -> bool {
        if self.from < self.to {
            (self.from..self.to).contains(&minute)
        } else {
            minute >= self.from || minute < self.to
        }
    }
}

/// What the manager knows about one queued download when deciding whether it may start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    /// Download hours are switched on in the settings.
    pub window_enabled: bool,
    /// The local time is inside them.
    pub window_open: bool,
    /// The user chose "download now" for this download: the window does not apply to it.
    pub start_now: bool,
    /// Not before this moment (unix seconds), set when the download was added.
    pub start_at: Option<i64>,
    /// When the hours end, running downloads finish instead of stopping.
    pub finish_running: bool,
}

impl Gate {
    /// May a waiting download start at unix time `now`? ("Finish" mode does not change this: it
    /// only lets what already runs go on.)
    pub fn may_start(&self, now: i64) -> bool {
        if self.start_at.is_some_and(|t| now < t) {
            return false;
        }
        self.hours_allow()
    }

    /// May a download that is already running go on? A set start time is behind it by then.
    pub fn may_run(&self) -> bool {
        self.hours_allow() || self.finish_running
    }

    fn hours_allow(&self) -> bool {
        !self.window_enabled || self.start_now || self.window_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn hm(h: u16, m: u16) -> u16 {
        h * 60 + m
    }

    #[test]
    fn a_window_inside_one_day() {
        let w = Window {
            from: hm(1, 0),
            to: hm(6, 0),
        };
        assert!(w.is_valid());
        assert!(!w.contains(hm(0, 59)));
        assert!(w.contains(hm(1, 0)));
        assert!(w.contains(hm(5, 59)));
        assert!(!w.contains(hm(6, 0)), "the end is exclusive");
    }

    #[test]
    fn a_window_over_midnight() {
        let w = Window {
            from: hm(22, 0),
            to: hm(6, 0),
        };
        assert!(w.is_valid());
        assert!(w.contains(hm(23, 30)));
        assert!(w.contains(hm(0, 0)));
        assert!(w.contains(hm(5, 59)));
        assert!(!w.contains(hm(6, 0)));
        assert!(!w.contains(hm(12, 0)));
        assert!(!w.contains(hm(21, 59)));
        assert!(w.contains(hm(22, 0)));
    }

    #[test]
    fn invalid_windows() {
        assert!(!Window { from: 100, to: 100 }.is_valid());
        assert!(!Window {
            from: DAY_MINUTES,
            to: 10
        }
        .is_valid());
        assert!(!Window {
            from: 10,
            to: DAY_MINUTES
        }
        .is_valid());
    }

    fn gate(enabled: bool, open: bool, now_flag: bool, at: Option<i64>) -> Gate {
        Gate {
            window_enabled: enabled,
            window_open: open,
            start_now: now_flag,
            start_at: at,
            finish_running: false,
        }
    }

    #[test]
    fn without_hours_everything_may_start() {
        assert!(gate(false, false, false, None).may_start(0));
    }

    #[test]
    fn hours_hold_downloads_outside_them_unless_started_now() {
        assert!(!gate(true, false, false, None).may_start(0));
        assert!(gate(true, true, false, None).may_start(0));
        assert!(
            gate(true, false, true, None).may_start(0),
            "download now ignores the hours"
        );
    }

    #[test]
    fn a_start_time_holds_until_it_comes() {
        let g = gate(false, true, false, Some(1000));
        assert!(!g.may_start(999));
        assert!(g.may_start(1000));
        // Both apply: a set time inside closed hours still waits for the hours.
        assert!(!gate(true, false, false, Some(1000)).may_start(2000));
    }

    #[test]
    fn a_running_download_only_cares_about_the_hours() {
        assert!(gate(false, false, false, Some(i64::MAX)).may_run());
        assert!(!gate(true, false, false, None).may_run());
        assert!(gate(true, false, true, None).may_run());
        assert!(gate(true, true, false, None).may_run());
    }

    #[test]
    fn finishing_lets_a_running_download_go_on_but_not_a_waiting_one_start() {
        let mut g = gate(true, false, false, None);
        g.finish_running = true;
        assert!(g.may_run(), "a running download is let finish");
        assert!(!g.may_start(0), "a waiting one still waits for the hours");
    }
}
