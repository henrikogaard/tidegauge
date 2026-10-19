//! What the Network page says about the connection: the default route's
//! interface and its type, the interface's addresses, Wi-Fi signal details
//! from CoreWLAN, and whether the Mac has a usable network path at all.
//!
//! Everything is read locally. The path comes from Network.framework's path
//! monitor, which sends no packets, and the Wi-Fi details leave out the
//! network's name and BSSID, which macOS only hands to apps with Location
//! permission, so nothing here can trigger a permission prompt.

use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use crate::ffi::*;
use crate::locale::text as l;
use crate::metrics::{cf_str, cf_to_string};

/// How long a Wi-Fi reading is reused. Signal and rate move slowly, and the
/// engine ticks every two seconds.
const WIFI_REFRESH: Duration = Duration::from_secs(6);

// ---- Link type ----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkKind {
    #[default]
    None,
    WiFi,
    Ethernet,
    /// A tunnel (`utun`, IPsec, PPP): a VPN or similar.
    Vpn,
    Other,
}

impl LinkKind {
    pub fn label(self) -> &'static str {
        match self {
            LinkKind::None => l("Not connected", "Ikke tilkoblet"),
            LinkKind::WiFi => "Wi-Fi",
            LinkKind::Ethernet => "Ethernet",
            LinkKind::Vpn => "VPN",
            LinkKind::Other => l("Other", "Annet"),
        }
    }
}

/// The type of the interface the default route uses. `wifi` is the name of
/// the Mac's Wi-Fi interface, which CoreWLAN knows even when it is not the
/// default route; an `en` interface that is not it is wired (or a dongle,
/// dock or phone tethered over a cable).
pub fn classify(name: &str, wifi: Option<&str>) -> LinkKind {
    if name.is_empty() {
        return LinkKind::None;
    }
    if wifi == Some(name) {
        return LinkKind::WiFi;
    }
    let prefix = name.trim_end_matches(|c: char| c.is_ascii_digit());
    match prefix {
        "en" => LinkKind::Ethernet,
        "utun" | "ipsec" | "ppp" | "tun" | "tap" | "gif" | "stf" => LinkKind::Vpn,
        _ => LinkKind::Other,
    }
}

// ---- Wi-Fi ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Ghz2,
    Ghz5,
    Ghz6,
}

impl Band {
    pub fn label(self) -> String {
        match self {
            Band::Ghz2 => format!("{} GHz", crate::locale::number(2.4, 1)),
            Band::Ghz5 => "5 GHz".into(),
            Band::Ghz6 => "6 GHz".into(),
        }
    }
}

/// The band of a channel. CoreWLAN names the band itself, which settles the
/// numbers that 5 and 6 GHz share; the channel number is only the fallback.
pub fn band_for(channel: u32, reported: Option<Band>) -> Option<Band> {
    reported.or(match channel {
        1..=14 => Some(Band::Ghz2),
        32..=177 => Some(Band::Ghz5),
        _ => None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Excellent,
    Good,
    Fair,
    Weak,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Excellent => l("Excellent", "Utmerket"),
            Quality::Good => l("Good", "God"),
            Quality::Fair => l("Fair", "Moderat"),
            Quality::Weak => l("Weak", "Svak"),
        }
    }
}

/// A plain word for a signal strength in dBm.
pub fn quality(rssi: i32) -> Quality {
    match rssi {
        -50.. => Quality::Excellent,
        -60..=-51 => Quality::Good,
        -70..=-61 => Quality::Fair,
        _ => Quality::Weak,
    }
}

/// The Wi-Fi standard behind a CoreWLAN PHY mode number.
fn phy_label(mode: isize) -> Option<&'static str> {
    Some(match mode {
        1 => "802.11a",
        2 => "802.11b",
        3 => "802.11g",
        4 => "802.11n",
        5 => "802.11ac",
        6 => "802.11ax",
        7 => "802.11be",
        _ => return None,
    })
}

/// One reading of the Mac's Wi-Fi interface while it is joined to a network.
/// A field macOS reports as zero is `None`: unknown, not zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wifi {
    pub rssi: i32,
    pub noise: Option<i32>,
    pub channel: Option<u32>,
    pub band: Option<Band>,
    pub rate_mbps: Option<f64>,
    pub standard: Option<&'static str>,
}

/// The Wi-Fi interface's name and, when it has joined a network, its reading.
fn read_wifi() -> WifiReading {
    use objc2_core_wlan::{CWChannelBand, CWWiFiClient};
    // SAFETY: plain property reads on CoreWLAN objects, no arguments.
    unsafe {
        let Some(interface) = CWWiFiClient::sharedWiFiClient().interface() else {
            return (None, None);
        };
        let name = interface.interfaceName().map(|n| n.to_string());
        let rssi = interface.rssiValue();
        // Zero is what an interface that is off or not joined reports.
        if !interface.powerOn() || rssi == 0 {
            return (name, None);
        }
        let noise = interface.noiseMeasurement();
        let rate = interface.transmitRate();
        let (channel, band) = match interface.wlanChannel() {
            Some(c) => {
                let number = c.channelNumber();
                let band = match c.channelBand() {
                    CWChannelBand::Band2GHz => Some(Band::Ghz2),
                    CWChannelBand::Band5GHz => Some(Band::Ghz5),
                    CWChannelBand::Band6GHz => Some(Band::Ghz6),
                    _ => None,
                };
                (u32::try_from(number).ok().filter(|n| *n > 0), band)
            }
            None => (None, None),
        };
        let wifi = Wifi {
            rssi: rssi as i32,
            noise: (noise != 0).then_some(noise as i32),
            channel,
            band: channel.and_then(|n| band_for(n, band)),
            rate_mbps: (rate.is_finite() && rate > 0.).then_some(rate),
            standard: phy_label(interface.activePHYMode().0),
        };
        (name, Some(wifi))
    }
}

/// The Wi-Fi interface's name and reading, as `read_wifi` returns them.
type WifiReading = (Option<String>, Option<Wifi>);

/// The last Wi-Fi reading and when it was taken.
static WIFI: Mutex<Option<(Instant, WifiReading)>> = Mutex::new(None);

fn wifi() -> WifiReading {
    let mut cached = WIFI.lock().unwrap();
    if let Some((at, reading)) = cached.as_ref() {
        if at.elapsed() < WIFI_REFRESH {
            return reading.clone();
        }
    }
    let reading = read_wifi();
    *cached = Some((Instant::now(), reading.clone()));
    reading
}

// ---- Default route and addresses ----

#[link(name = "SystemConfiguration", kind = "framework")]
extern "C" {
    fn SCDynamicStoreCreate(
        allocator: CFAllocatorRef,
        name: CFStringRef,
        callout: *const c_void,
        context: *const c_void,
    ) -> CFTypeRef;
    fn SCDynamicStoreCopyValue(store: CFTypeRef, key: CFStringRef) -> CFTypeRef;
}

/// The interface of the default route, as the system configuration store
/// names it, or `None` without one. IPv4 first, then IPv6.
pub fn default_interface() -> Option<String> {
    // SAFETY: every Core Foundation object created here is released; the
    // dictionary value is only read while the dictionary is alive.
    unsafe {
        let name = cf_str("tidegauge");
        let store =
            SCDynamicStoreCreate(std::ptr::null(), name, std::ptr::null(), std::ptr::null());
        CFRelease(name);
        if store.is_null() {
            return None;
        }
        let field = cf_str("PrimaryInterface");
        let mut found = None;
        for key in ["State:/Network/Global/IPv4", "State:/Network/Global/IPv6"] {
            let key = cf_str(key);
            let dict = SCDynamicStoreCopyValue(store, key);
            CFRelease(key);
            if dict.is_null() {
                continue;
            }
            if CFGetTypeID(dict) == CFDictionaryGetTypeID() {
                found = cf_to_string(CFDictionaryGetValue(dict, field)).filter(|n| !n.is_empty());
            }
            CFRelease(dict);
            if found.is_some() {
                break;
            }
        }
        CFRelease(field);
        CFRelease(store);
        found
    }
}

/// An interface's IPv4 and IPv6 addresses. Link-local addresses (169.254/16
/// and fe80::/10) are skipped: they say nothing about a working network.
pub fn addresses(interface: &str) -> (Option<String>, Option<String>) {
    let (mut v4, mut v6) = (None, None);
    // SAFETY: the list from getifaddrs is walked once and freed; socket
    // addresses are read at the offsets their family defines, after the
    // family says which layout it is.
    unsafe {
        let mut list: *mut ifaddrs = std::ptr::null_mut();
        if getifaddrs(&mut list) != 0 || list.is_null() {
            return (None, None);
        }
        let mut cur = list;
        while !cur.is_null() {
            let a = &*cur;
            cur = a.ifa_next;
            if a.ifa_addr.is_null() || a.ifa_name.is_null() {
                continue;
            }
            if std::ffi::CStr::from_ptr(a.ifa_name).to_bytes() != interface.as_bytes() {
                continue;
            }
            let base = a.ifa_addr as *const u8;
            match (*a.ifa_addr).sa_family {
                AF_INET if v4.is_none() => {
                    // sockaddr_in: len, family, port (2), address (4).
                    let octets = [*base.add(4), *base.add(5), *base.add(6), *base.add(7)];
                    let ip = std::net::Ipv4Addr::from(octets);
                    if !ip.is_link_local() && !ip.is_unspecified() {
                        v4 = Some(ip.to_string());
                    }
                }
                AF_INET6 if v6.is_none() => {
                    // sockaddr_in6: len, family, port (2), flow info (4), address (16).
                    let mut octets = [0u8; 16];
                    std::ptr::copy_nonoverlapping(base.add(8), octets.as_mut_ptr(), 16);
                    let ip = std::net::Ipv6Addr::from(octets);
                    if !is_link_local_v6(&ip) && !ip.is_unspecified() {
                        v6 = Some(ip.to_string());
                    }
                }
                _ => {}
            }
        }
        freeifaddrs(list);
    }
    (v4, v6)
}

fn is_link_local_v6(ip: &std::net::Ipv6Addr) -> bool {
    ip.segments()[0] & 0xffc0 == 0xfe80
}

// ---- Connectivity ----

const PATH_UNKNOWN: u8 = 0;
const PATH_ONLINE: u8 = 1;
const PATH_OFFLINE: u8 = 2;

/// The latest status the path monitor reported.
static PATH: AtomicU8 = AtomicU8::new(PATH_UNKNOWN);
static MONITOR: Once = Once::new();

/// `nw_path_status_satisfied`: the Mac has an interface that can reach out.
const NW_PATH_SATISFIED: i32 = 1;

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_path_monitor_create() -> *mut c_void;
    fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
    fn nw_path_monitor_set_update_handler(monitor: *mut c_void, handler: *const c_void);
    fn nw_path_monitor_start(monitor: *mut c_void);
    fn nw_path_get_status(path: *mut c_void) -> i32;
}

extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
}

/// Start watching the system's network path, once. macOS calls the handler
/// at once with the current path and again whenever it changes; the monitor
/// runs for the life of the app.
fn start_monitor() {
    MONITOR.call_once(|| {
        let handler = block2::RcBlock::new(|path: *mut c_void| {
            // SAFETY: Network.framework passes a live path for the call.
            let status = unsafe { nw_path_get_status(path) };
            PATH.store(
                if status == NW_PATH_SATISFIED {
                    PATH_ONLINE
                } else {
                    PATH_OFFLINE
                },
                Ordering::Relaxed,
            );
        });
        let label = CString::new("no.ogard.tidegauge.path").unwrap();
        // SAFETY: the monitor and queue are created here and kept for the
        // life of the process; the framework copies the handler block.
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                return;
            }
            let queue = dispatch_queue_create(label.as_ptr(), std::ptr::null());
            nw_path_monitor_set_queue(monitor, queue);
            nw_path_monitor_set_update_handler(monitor, &*handler as *const _ as *const c_void);
            nw_path_monitor_start(monitor);
        }
    });
}

/// Whether the Mac can reach a network. The path monitor decides; until it
/// has reported (or if it cannot start), an interface with a default route
/// and an address counts as online.
pub fn is_online(has_route_and_address: bool) -> bool {
    start_monitor();
    match PATH.load(Ordering::Relaxed) {
        PATH_ONLINE => true,
        PATH_OFFLINE => false,
        _ => has_route_and_address,
    }
}

// ---- Snapshot ----

/// Everything the Network page shows besides traffic.
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub kind: LinkKind,
    pub interface: String,
    pub online: bool,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub wifi: Option<Wifi>,
}

pub fn details() -> Details {
    let (wifi_name, wifi) = wifi();
    let interface = default_interface().unwrap_or_default();
    let (ipv4, ipv6) = if interface.is_empty() {
        (None, None)
    } else {
        addresses(&interface)
    };
    let usable = !interface.is_empty() && (ipv4.is_some() || ipv6.is_some());
    let kind = classify(&interface, wifi_name.as_deref());
    Details {
        kind,
        online: is_online(usable),
        interface,
        ipv4,
        ipv6,
        wifi: primary_wifi(kind, wifi),
    }
}

/// Wi-Fi details describe the connection only when Wi-Fi carries it; on
/// Ethernet or a VPN they would belong to a link that is not in use.
fn primary_wifi(kind: LinkKind, wifi: Option<Wifi>) -> Option<Wifi> {
    wifi.filter(|_| kind == LinkKind::WiFi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_strength_gets_a_plain_word() {
        assert_eq!(quality(-30), Quality::Excellent);
        assert_eq!(quality(-50), Quality::Excellent);
        assert_eq!(quality(-51), Quality::Good);
        assert_eq!(quality(-60), Quality::Good);
        assert_eq!(quality(-61), Quality::Fair);
        assert_eq!(quality(-70), Quality::Fair);
        assert_eq!(quality(-71), Quality::Weak);
        assert_eq!(quality(-95), Quality::Weak);
    }

    #[test]
    fn channels_map_to_bands() {
        assert_eq!(band_for(1, None), Some(Band::Ghz2));
        assert_eq!(band_for(11, None), Some(Band::Ghz2));
        assert_eq!(band_for(14, None), Some(Band::Ghz2));
        assert_eq!(band_for(36, None), Some(Band::Ghz5));
        assert_eq!(band_for(149, None), Some(Band::Ghz5));
        assert_eq!(band_for(0, None), None);
        assert_eq!(band_for(200, None), None);
        // Channel 37 exists in both 5 and 6 GHz; the reported band wins.
        assert_eq!(band_for(37, Some(Band::Ghz6)), Some(Band::Ghz6));
        assert_eq!(band_for(5, Some(Band::Ghz6)), Some(Band::Ghz6));
    }

    #[test]
    fn wifi_details_follow_only_a_wifi_connection() {
        let wifi = || Some(Wifi::default());
        assert!(primary_wifi(LinkKind::WiFi, wifi()).is_some());
        assert!(primary_wifi(LinkKind::Ethernet, wifi()).is_none());
        assert!(primary_wifi(LinkKind::Vpn, wifi()).is_none());
        assert!(primary_wifi(LinkKind::None, wifi()).is_none());
    }

    #[test]
    fn interfaces_are_classified_by_what_they_are() {
        assert_eq!(classify("en0", Some("en0")), LinkKind::WiFi);
        assert_eq!(classify("en5", Some("en0")), LinkKind::Ethernet);
        assert_eq!(classify("en0", None), LinkKind::Ethernet);
        assert_eq!(classify("utun4", Some("en0")), LinkKind::Vpn);
        assert_eq!(classify("ipsec0", None), LinkKind::Vpn);
        assert_eq!(classify("ppp0", None), LinkKind::Vpn);
        assert_eq!(classify("bridge0", Some("en0")), LinkKind::Other);
        assert_eq!(classify("pdp_ip0", None), LinkKind::Other);
        assert_eq!(classify("", Some("en0")), LinkKind::None);
        // "en" must be the whole prefix: "enc0" is not Ethernet.
        assert_eq!(classify("enc0", None), LinkKind::Other);
    }

    #[test]
    fn phy_modes_have_standard_names() {
        assert_eq!(phy_label(6), Some("802.11ax"));
        assert_eq!(phy_label(0), None);
        assert_eq!(phy_label(99), None);
    }

    #[test]
    fn link_local_addresses_are_recognised() {
        assert!(is_link_local_v6(&"fe80::1".parse().unwrap()));
        assert!(is_link_local_v6(&"febf::1".parse().unwrap()));
        assert!(!is_link_local_v6(&"2001:db8::1".parse().unwrap()));
        assert!(!is_link_local_v6(&"fd00::1".parse().unwrap()));
    }

    #[test]
    fn addresses_are_read_from_the_named_interface() {
        let (v4, _) = addresses("lo0");
        assert_eq!(v4.as_deref(), Some("127.0.0.1"));
    }

    /// Reads this Mac's real connection and prints it, to compare with
    /// System Settings and `ifconfig`. Asserts only what must hold.
    #[test]
    #[ignore = "reads this Mac's network"]
    fn native_connection_details_are_readable() {
        let first = details();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let again = details();
        println!("{again:#?}");
        assert_eq!(first.interface, again.interface);
        assert_ne!(
            PATH.load(Ordering::Relaxed),
            PATH_UNKNOWN,
            "the path monitor reported"
        );
        if again.online && !again.interface.is_empty() {
            assert!(again.ipv4.is_some() || again.ipv6.is_some() || again.kind == LinkKind::Vpn);
        }
    }
}
