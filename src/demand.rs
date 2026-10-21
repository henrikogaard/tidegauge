//! What is on screen, so probes that only feed a view can rest while nothing
//! shows them. The dashboard and the menu-bar panel report themselves here; the
//! sampler and the probe threads read it to pick a cadence. Alerts keep the
//! probes they need running whatever is on screen.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// The dashboard pages that need a probe of their own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Other,
    Cpu,
    Disk,
    Network,
    Bluetooth,
    Projects,
    Agents,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    /// The dashboard's page while its window is on screen.
    pub window: Option<Page>,
    /// The menu-bar panel is open.
    pub panel: bool,
    /// The network alert is on; it names the busiest app.
    pub network_alert: bool,
    /// The disk-health alert is on.
    pub disk_health_alert: bool,
}

/// The panel reports itself every second; a longer silence means it is gone.
const PANEL_SEEN_FOR: Duration = Duration::from_secs(3);

/// Process-level sampling while something is on screen, and while nothing is.
/// Hidden stays under the 10 s limit of the activity trackers.
const WATCHED_PERIOD: Duration = Duration::from_secs(2);
const HIDDEN_PERIOD: Duration = Duration::from_secs(8);
/// A pass this much early still counts as due, so tick jitter never skips a
/// whole period.
const JITTER: Duration = Duration::from_secs(1);
/// How often the IOReport readings (clock speeds, GPU power) are taken while
/// nothing shows them, and the longest gap whose reading still counts as
/// current: a longer one is an average of the gap, so it is only a baseline.
const HIDDEN_RATES_PERIOD: Duration = Duration::from_secs(20);
const RATES_MAX_GAP: Duration = Duration::from_secs(30);
/// How old the audio list may get while nothing shows it.
const HIDDEN_AUDIO_PERIOD: Duration = Duration::from_secs(30);

struct State {
    window: Option<Page>,
    panel_seen: Option<Instant>,
    network_alert: bool,
    disk_health_alert: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    window: None,
    panel_seen: None,
    network_alert: false,
    disk_health_alert: false,
});

/// Counts the changes to the demand, so a worker that rests between probes
/// can sleep until one instead of waking to look.
static CHANGES: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

fn changed() {
    *CHANGES.0.lock().unwrap() += 1;
    CHANGES.1.notify_all();
}

/// The number of changes so far. Read it before reading the demand, then pass
/// it to [`wait_for_change`]: a change in between is not missed.
pub fn generation() -> u64 {
    *CHANGES.0.lock().unwrap()
}

/// Sleep until the demand changes after `seen`, or `timeout` passes.
pub fn wait_for_change(seen: u64, timeout: Duration) {
    let guard = CHANGES.0.lock().unwrap();
    let _ = CHANGES
        .1
        .wait_timeout_while(guard, timeout, |now| *now == seen);
}

pub fn set_window(page: Option<Page>) {
    let mut state = STATE.lock().unwrap();
    if state.window != page {
        state.window = page;
        drop(state);
        changed();
    }
}

/// The panel is open right now.
pub fn panel_seen() {
    let mut state = STATE.lock().unwrap();
    let was_open = state
        .panel_seen
        .is_some_and(|at| at.elapsed() < PANEL_SEEN_FOR);
    state.panel_seen = Some(Instant::now());
    if !was_open {
        drop(state);
        changed();
    }
}

pub fn set_alerts(network: bool, disk_health: bool) {
    let mut state = STATE.lock().unwrap();
    if (state.network_alert, state.disk_health_alert) != (network, disk_health) {
        state.network_alert = network;
        state.disk_health_alert = disk_health;
        drop(state);
        changed();
    }
}

pub fn current() -> Demand {
    let state = STATE.lock().unwrap();
    Demand {
        window: state.window,
        panel: state
            .panel_seen
            .is_some_and(|at| at.elapsed() < PANEL_SEEN_FOR),
        network_alert: state.network_alert,
        disk_health_alert: state.disk_health_alert,
    }
}

impl Demand {
    /// A window or the panel is on screen.
    pub fn watched(&self) -> bool {
        self.window.is_some() || self.panel
    }

    fn page(&self, page: Page) -> bool {
        self.window == Some(page)
    }

    /// The process list, apps and agents: every tick while watched.
    pub fn heavy_period(&self) -> Duration {
        if self.watched() {
            WATCHED_PERIOD
        } else {
            HIDDEN_PERIOD
        }
    }

    /// Whether the process pass is due, `since_last` after the previous one.
    pub fn heavy_due(&self, since_last: Duration) -> bool {
        since_last + JITTER >= self.heavy_period()
    }

    /// How often `nettop` runs: per-app network is shown on the Network page,
    /// the network alert names the busiest app, and history keeps a coarse
    /// record otherwise.
    pub fn nettop_period(&self) -> Duration {
        if self.page(Page::Network) {
            Duration::from_secs(4)
        } else if self.network_alert {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(60)
        }
    }

    /// How often `diskutil` runs; `None` after the first run when nothing
    /// shows or checks the inventory. Every ten minutes while the dashboard
    /// is open or the disk-health alert is on, every 30 s on the Disk page.
    pub fn inventory_period(&self) -> Option<Duration> {
        if self.page(Page::Disk) {
            Some(Duration::from_secs(30))
        } else if self.disk_health_alert || self.window.is_some() {
            // The alert watches in the background; the dashboard's Needs
            // attention strip shows a failing disk while the window is open.
            Some(Duration::from_secs(600))
        } else {
            None
        }
    }

    /// How old Bluetooth data may get: 20 s on its page, otherwise only the
    /// sidebar count needs it.
    pub fn bluetooth_period(&self) -> Duration {
        if self.page(Page::Bluetooth) {
            Duration::from_secs(20)
        } else {
            Duration::from_secs(300)
        }
    }

    /// How old the list of listening ports may get: fresh where dev
    /// servers are listed, otherwise only counts and badges use it.
    pub fn listeners_period(&self) -> Duration {
        if self.page(Page::Projects) || self.page(Page::Agents) {
            Duration::ZERO
        } else {
            Duration::from_secs(30)
        }
    }

    /// Per-disk transfer rates are drawn on the dashboard's Disk page, with
    /// the last two minutes behind them, so they are kept while a window is
    /// open.
    pub fn disk_io_wanted(&self) -> bool {
        self.window.is_some()
    }

    /// Whether the IOReport readings are due, `since_last` after the previous
    /// one. Each costs a few milliseconds of CPU, and only the dashboard
    /// shows them, so with nothing on screen they are taken now and then.
    pub fn rates_due(&self, since_last: Duration) -> bool {
        self.watched() || since_last + JITTER >= HIDDEN_RATES_PERIOD
    }

    /// Whether a reading taken `since_last` after the previous one describes
    /// now, or is the average of a long gap that is better left unshown.
    pub fn rates_current(since_last: Duration) -> bool {
        since_last <= RATES_MAX_GAP
    }

    /// Whether the audio apps and devices are due, `since_last` after the
    /// previous pass. Something on screen shows them, and a tap on an app
    /// must follow it and the default output; otherwise a slow look keeps
    /// them from being long out of date.
    pub fn audio_due(&self, since_last: Duration, taps: bool) -> bool {
        self.watched() || taps || since_last >= HIDDEN_AUDIO_PERIOD
    }

    /// The raw AppleSMC temperature channels are listed on the CPU page only.
    pub fn raw_sensors(&self) -> bool {
        self.page(Page::Cpu)
    }
}

/// Run the calling worker thread at utility quality of service, so macOS
/// favors efficiency cores for it.
pub fn utility_thread() {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(page: Page) -> Demand {
        Demand {
            window: Some(page),
            ..Default::default()
        }
    }

    #[test]
    fn audio_rests_while_nothing_shows_it_and_no_app_is_tapped() {
        let hidden = Demand::default();
        assert!(!hidden.audio_due(Duration::from_secs(2), false));
        assert!(!hidden.audio_due(Duration::from_secs(29), false));
        assert!(hidden.audio_due(Duration::from_secs(30), false));
        assert!(
            hidden.audio_due(Duration::from_secs(2), true),
            "taps follow their app"
        );
        assert!(window(Page::Other).audio_due(Duration::ZERO, false));
        let panel = Demand {
            panel: true,
            ..Default::default()
        };
        assert!(panel.audio_due(Duration::ZERO, false));
    }

    #[test]
    fn iokit_readings_rest_while_nothing_shows_them() {
        let hidden = Demand::default();
        assert!(!hidden.rates_due(Duration::from_secs(2)));
        assert!(!hidden.rates_due(Duration::from_secs(18)));
        assert!(hidden.rates_due(Duration::from_secs(19)));
        assert!(window(Page::Cpu).rates_due(Duration::from_secs(2)));
        assert!(Demand {
            panel: true,
            ..Default::default()
        }
        .rates_due(Duration::ZERO));
        assert!(Demand::rates_current(Duration::from_secs(22)));
        assert!(!Demand::rates_current(Duration::from_secs(31)));
    }

    #[test]
    fn disk_rates_are_kept_only_for_an_open_window() {
        assert!(!Demand::default().disk_io_wanted());
        assert!(!Demand {
            panel: true,
            ..Default::default()
        }
        .disk_io_wanted());
        assert!(window(Page::Other).disk_io_wanted());
    }

    #[test]
    fn a_waiting_worker_wakes_on_a_change_not_on_the_timeout() {
        let seen = generation();
        let waiter = std::thread::spawn(move || {
            let start = Instant::now();
            wait_for_change(seen, Duration::from_secs(30));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(50));
        changed();
        assert!(waiter.join().unwrap() < Duration::from_secs(5));
        // A change made before the wait began is not slept through.
        let start = Instant::now();
        wait_for_change(seen, Duration::from_secs(30));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn hidden_slows_the_process_pass_but_stays_under_the_tracker_limit() {
        let hidden = Demand::default();
        assert!(!hidden.watched());
        assert_eq!(hidden.heavy_period(), Duration::from_secs(8));
        assert!(!hidden.heavy_due(Duration::from_secs(2)));
        assert!(!hidden.heavy_due(Duration::from_secs(6)));
        assert!(hidden.heavy_due(Duration::from_secs(7)));
        assert!(hidden.heavy_due(Duration::from_secs(8)));
    }

    #[test]
    fn a_window_or_the_panel_keeps_the_process_pass_every_tick() {
        for demand in [
            window(Page::Other),
            Demand {
                panel: true,
                ..Default::default()
            },
        ] {
            assert!(demand.watched());
            assert!(demand.heavy_due(Duration::from_millis(1900)));
        }
    }

    #[test]
    fn nettop_runs_often_only_where_it_is_shown_or_alerted() {
        assert_eq!(
            window(Page::Network).nettop_period(),
            Duration::from_secs(4)
        );
        assert_eq!(window(Page::Cpu).nettop_period(), Duration::from_secs(60));
        assert_eq!(Demand::default().nettop_period(), Duration::from_secs(60));
        let alert = Demand {
            network_alert: true,
            ..Default::default()
        };
        assert_eq!(alert.nettop_period(), Duration::from_secs(10));
        let both = Demand {
            network_alert: true,
            ..window(Page::Network)
        };
        assert_eq!(both.nettop_period(), Duration::from_secs(4));
    }

    #[test]
    fn the_disk_inventory_follows_its_page_and_its_alert() {
        assert_eq!(
            window(Page::Disk).inventory_period(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(Demand::default().inventory_period(), None);
        // Any open dashboard page refreshes slowly, for the attention strip.
        assert_eq!(
            window(Page::Network).inventory_period(),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            window(Page::Other).inventory_period(),
            Some(Duration::from_secs(600))
        );
        let panel = Demand {
            panel: true,
            ..Default::default()
        };
        assert_eq!(panel.inventory_period(), None);
        let alert = Demand {
            disk_health_alert: true,
            ..Default::default()
        };
        assert_eq!(alert.inventory_period(), Some(Duration::from_secs(600)));
        let both = Demand {
            disk_health_alert: true,
            ..window(Page::Disk)
        };
        assert_eq!(both.inventory_period(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn bluetooth_listeners_and_raw_sensors_wait_for_their_pages() {
        assert_eq!(
            window(Page::Bluetooth).bluetooth_period(),
            Duration::from_secs(20)
        );
        assert_eq!(
            window(Page::Other).bluetooth_period(),
            Duration::from_secs(300)
        );
        assert_eq!(
            Demand::default().bluetooth_period(),
            Duration::from_secs(300)
        );
        assert_eq!(window(Page::Projects).listeners_period(), Duration::ZERO);
        assert_eq!(window(Page::Agents).listeners_period(), Duration::ZERO);
        assert_eq!(
            window(Page::Other).listeners_period(),
            Duration::from_secs(30)
        );
        let panel = Demand {
            panel: true,
            ..Default::default()
        };
        assert_eq!(panel.listeners_period(), Duration::from_secs(30));
        assert!(window(Page::Cpu).raw_sensors());
        assert!(!window(Page::Disk).raw_sensors());
        assert!(!Demand::default().raw_sensors());
    }
}
