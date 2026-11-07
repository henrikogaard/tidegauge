//! Latency and loss on the link to the router, for the Network page.
//!
//! One ICMP echo goes to the default gateway per pass, and only while the
//! Network page is open. The target is the next hop on the local link: a
//! gateway outside private, carrier-grade and link-local ranges, a tunnel's
//! default route, or no default route at all means nothing is sent. The probe
//! uses the unprivileged ICMP datagram socket macOS allows without root and
//! waits at most one second, on a thread of its own, so the sampler never
//! waits on it. It measures the link to the router, not the internet.

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Probes the strip keeps.
const HISTORY: usize = 60;
/// Probes behind the average, worst and loss figures.
const WINDOW: usize = 30;
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const PROBE_PERIOD: Duration = Duration::from_secs(2);
/// The gateway is looked up again after this many probes, or after a loss.
const RESOLVE_EVERY: u32 = 5;
const IDENT: u16 = 0x7467;
const ROUTE_TIMEOUT: Duration = Duration::from_secs(1);

static ROUTE: crate::bounded::Probe = crate::bounded::Probe::new();

/// Round-trip times in milliseconds, oldest first; `None` is a lost probe.
#[derive(Clone, Debug, Default)]
pub struct Ring {
    samples: VecDeque<Option<f64>>,
}

/// The figures over the last [`WINDOW`] probes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub current: Option<f64>,
    pub average: Option<f64>,
    pub worst: Option<f64>,
    pub loss_pct: f64,
    pub probes: usize,
}

impl Ring {
    pub fn push(&mut self, rtt: Option<f64>) {
        if self.samples.len() == HISTORY {
            self.samples.pop_front();
        }
        self.samples.push_back(rtt);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The probes behind [`Ring::stats`], for a chart; a lost one is NaN,
    /// which draws as a gap.
    pub fn series(&self) -> Vec<f64> {
        let start = self.samples.len().saturating_sub(WINDOW);
        self.samples.iter().skip(start).map(|s| s.unwrap_or(f64::NAN)).collect()
    }

    pub fn stats(&self) -> Option<Stats> {
        let start = self.samples.len().saturating_sub(WINDOW);
        let recent: Vec<Option<f64>> = self.samples.iter().skip(start).copied().collect();
        if recent.is_empty() {
            return None;
        }
        let answered: Vec<f64> = recent.iter().flatten().copied().collect();
        let lost = recent.len() - answered.len();
        Some(Stats {
            current: *recent.last().unwrap(),
            average: (!answered.is_empty()).then(|| answered.iter().sum::<f64>() / answered.len() as f64),
            worst: answered.iter().copied().reduce(f64::max),
            loss_pct: lost as f64 * 100.0 / recent.len() as f64,
            probes: recent.len(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Nothing has been probed: the page was not open, or not yet.
    #[default]
    Idle,
    /// No usable default gateway: offline, a tunnel, or not a local address.
    NoGateway,
    Probing(Ipv4Addr),
    /// The probe cannot run here (the socket or the send failed): not a loss.
    Unavailable(Ipv4Addr),
}

/// What one echo came to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    /// Answered after this many milliseconds.
    Reply(f64),
    /// Sent, but nothing came back in time: a lost probe.
    Lost,
    /// Never sent, because of a local error: says nothing about the router.
    Unavailable,
}

#[derive(Clone, Default)]
pub struct View {
    pub status: Status,
    pub stats: Option<Stats>,
    pub series: Vec<f64>,
}

struct State {
    status: Status,
    ring: Ring,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: Status::Idle,
    ring: Ring {
        samples: VecDeque::new(),
    },
});

/// What the Network page shows.
pub fn view() -> View {
    let s = STATE.lock().unwrap();
    View {
        status: s.status.clone(),
        stats: s.ring.stats(),
        series: s.ring.series(),
    }
}

// ---- Gateway ----

/// The gateway from `route -n get default`, when it is an IPv4 next hop on a
/// local link that is not a tunnel.
pub fn parse_route(text: &str) -> Option<Ipv4Addr> {
    let mut gateway = None;
    let mut interface = "";
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        match key.trim() {
            "gateway" => gateway = value.trim().parse::<Ipv4Addr>().ok(),
            "interface" => interface = value.trim(),
            _ => {}
        }
    }
    let gateway = gateway?;
    if crate::network::classify(interface, None) == crate::network::LinkKind::Vpn || !is_local(gateway) {
        return None;
    }
    Some(gateway)
}

/// Private, carrier-grade NAT and link-local addresses: a next hop that is
/// the router, never a host out on the internet.
fn is_local(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1]))
}

fn find_gateway() -> Option<Ipv4Addr> {
    let mut command = std::process::Command::new("/sbin/route");
    command.args(["-n", "get", "default"]);
    let out = ROUTE.run(command, ROUTE_TIMEOUT)?;
    parse_route(&String::from_utf8_lossy(&out))
}

// ---- ICMP ----

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in data.chunks(2) {
        sum += u32::from(pair[0]) << 8 | u32::from(*pair.get(1).unwrap_or(&0));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn echo_request(ident: u16, seq: u16) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = 8;
    p[4..6].copy_from_slice(&ident.to_be_bytes());
    p[6..8].copy_from_slice(&seq.to_be_bytes());
    p[8..].copy_from_slice(b"tidegaug");
    let sum = checksum(&p);
    p[2..4].copy_from_slice(&sum.to_be_bytes());
    p
}

/// The identifier of `packet` when it is the echo reply to `seq`. The
/// datagram socket may hand the reply back with its IP header in front.
fn reply_ident(packet: &[u8], seq: u16) -> Option<u16> {
    let icmp = if packet.first().is_some_and(|b| b >> 4 == 4) {
        let header = usize::from(packet[0] & 0x0f) * 4;
        packet.get(header..).unwrap_or(&[])
    } else {
        packet
    };
    (icmp.len() >= 8 && icmp[0] == 0 && u16::from_be_bytes([icmp[6], icmp[7]]) == seq)
        .then(|| u16::from_be_bytes([icmp[4], icmp[5]]))
}

/// One echo to `target`, with the identifier its reply carried.
fn ping(target: Ipv4Addr, seq: u16) -> (Outcome, Option<u16>) {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP);
        if fd < 0 {
            return (Outcome::Unavailable, None);
        }
        let result = (|| {
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_addr.s_addr = u32::from_ne_bytes(target.octets());
            let packet = echo_request(IDENT, seq);
            let started = Instant::now();
            let sent = libc::sendto(
                fd,
                packet.as_ptr().cast(),
                packet.len(),
                0,
                (&addr as *const libc::sockaddr_in).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            if sent < 0 {
                // No route, no buffer space, a firewall: the probe never left.
                return (Outcome::Unavailable, None);
            }
            let mut buf = [0u8; 128];
            loop {
                let Some(left) = PROBE_TIMEOUT.checked_sub(started.elapsed()) else {
                    return (Outcome::Lost, None);
                };
                let tv = libc::timeval {
                    tv_sec: 0,
                    tv_usec: left.as_micros().max(1) as libc::suseconds_t,
                };
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVTIMEO,
                    (&tv as *const libc::timeval).cast(),
                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                );
                let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
                if n < 0 {
                    match std::io::Error::last_os_error().kind() {
                        // A signal is not a loss: wait out the rest.
                        std::io::ErrorKind::Interrupted => continue,
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => return (Outcome::Lost, None),
                        _ => return (Outcome::Unavailable, None),
                    }
                }
                // XNU keeps the identifier we sent on a datagram socket, so a
                // reply must carry both it and the sequence.
                if let Some(ident) = reply_ident(&buf[..n as usize], seq).filter(|id| *id == IDENT) {
                    return (Outcome::Reply(started.elapsed().as_secs_f64() * 1000.0), Some(ident));
                }
            }
        })();
        libc::close(fd);
        result
    }
}

// ---- Worker ----

/// Forget everything: what was measured on one network says nothing about
/// the next one the page is opened on.
fn reset() {
    let mut s = STATE.lock().unwrap();
    s.status = Status::Idle;
    s.ring.clear();
}

/// Start the probe thread. It rests until the Network page opens.
pub fn start() {
    let _ = std::thread::Builder::new()
        .name("tidegauge-router-ping".into())
        .spawn(|| {
            crate::demand::utility_thread();
            let mut gateway: Option<Ipv4Addr> = None;
            let (mut since_resolve, mut seq) = (RESOLVE_EVERY, 0u16);
            let mut last: Option<Instant> = None;
            loop {
                let seen = crate::demand::generation();
                if !crate::demand::current().router_probe_wanted() {
                    gateway = None;
                    since_resolve = RESOLVE_EVERY;
                    reset();
                    crate::demand::wait_for_change(seen, Duration::from_secs(60));
                    continue;
                }
                // Any change in the demand wakes this thread; a probe is
                // still never sent sooner than PROBE_PERIOD after the last.
                if let Some(wait) = last.and_then(|at| PROBE_PERIOD.checked_sub(at.elapsed())) {
                    crate::demand::wait_for_change(seen, wait);
                    continue;
                }
                last = Some(Instant::now());
                if gateway.is_none() || since_resolve >= RESOLVE_EVERY {
                    gateway = find_gateway();
                    since_resolve = 0;
                }
                since_resolve += 1;
                match gateway {
                    None => {
                        let mut s = STATE.lock().unwrap();
                        s.status = Status::NoGateway;
                        s.ring.clear();
                    }
                    Some(ip) => {
                        seq = seq.wrapping_add(1);
                        let outcome = ping(ip, seq).0;
                        if !matches!(outcome, Outcome::Reply(_)) {
                            since_resolve = RESOLVE_EVERY;
                        }
                        let mut s = STATE.lock().unwrap();
                        match outcome {
                            Outcome::Reply(ms) => {
                                s.status = Status::Probing(ip);
                                s.ring.push(Some(ms));
                            }
                            Outcome::Lost => {
                                s.status = Status::Probing(ip);
                                s.ring.push(None);
                            }
                            Outcome::Unavailable => s.status = Status::Unavailable(ip),
                        }
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(values: &[Option<f64>]) -> Ring {
        let mut r = Ring::default();
        for v in values {
            r.push(*v);
        }
        r
    }

    #[test]
    fn stats_cover_loss_average_and_worst() {
        let s = ring(&[Some(2.0), None, Some(4.0), Some(12.0)]).stats().unwrap();
        assert_eq!(s.current, Some(12.0));
        assert_eq!(s.average, Some(6.0));
        assert_eq!(s.worst, Some(12.0));
        assert_eq!(s.loss_pct, 25.0);
        assert_eq!(s.probes, 4);
    }

    #[test]
    fn all_lost_has_no_latency_and_empty_ring_has_no_stats() {
        assert!(Ring::default().stats().is_none());
        let s = ring(&[None, None]).stats().unwrap();
        assert_eq!((s.current, s.average, s.worst, s.loss_pct), (None, None, None, 100.0));
    }

    #[test]
    fn ring_keeps_history_but_stats_use_the_last_window() {
        let mut r = Ring::default();
        for _ in 0..HISTORY {
            r.push(None);
        }
        for _ in 0..WINDOW {
            r.push(Some(1.0));
        }
        assert_eq!(r.samples.len(), HISTORY);
        let s = r.stats().unwrap();
        assert_eq!((s.loss_pct, s.probes), (0.0, WINDOW));
        let series = r.series();
        assert_eq!(series.len(), WINDOW, "the strip covers the same probes as the figures");
        assert!(series.iter().all(|v| *v == 1.0));
        r.clear();
        assert!(r.series().is_empty());
    }

    #[test]
    fn reset_forgets_probes_and_status() {
        {
            let mut s = STATE.lock().unwrap();
            s.status = Status::Probing(Ipv4Addr::new(192, 168, 1, 1));
            s.ring.push(Some(3.0));
        }
        reset();
        let v = view();
        assert_eq!(v.status, Status::Idle);
        assert!(v.stats.is_none() && v.series.is_empty());
    }

    const ROUTE_OK: &str = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.50.1\n  interface: en0\n      flags: <UP>\n";

    #[test]
    fn parses_a_local_gateway() {
        assert_eq!(parse_route(ROUTE_OK), Some(Ipv4Addr::new(192, 168, 50, 1)));
    }

    #[test]
    fn refuses_tunnels_public_and_missing_gateways() {
        assert_eq!(parse_route(&ROUTE_OK.replace("en0", "utun4")), None);
        assert_eq!(parse_route(&ROUTE_OK.replace("192.168.50.1", "8.8.8.8")), None);
        assert_eq!(parse_route(&ROUTE_OK.replace("192.168.50.1", "link#4")), None);
        assert_eq!(parse_route("route: writing to routing socket: not in table\n"), None);
        assert_eq!(parse_route(&ROUTE_OK.replace("192.168.50.1", "100.72.0.1")), Some(Ipv4Addr::new(100, 72, 0, 1)));
        assert_eq!(parse_route(&ROUTE_OK.replace("192.168.50.1", "100.128.0.1")), None);
    }

    #[test]
    fn echo_request_has_a_valid_checksum_and_replies_match_by_sequence() {
        let p = echo_request(1, 7);
        assert_eq!(p[0], 8);
        assert_eq!(checksum(&p), 0);
        let mut reply = p;
        reply[0] = 0;
        assert_eq!(reply_ident(&reply, 7), Some(1));
        assert_eq!(reply_ident(&reply, 8), None);
        let mut with_ip = vec![0x45u8, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2];
        with_ip.extend_from_slice(&reply);
        assert_eq!(reply_ident(&with_ip, 7), Some(1));
        assert_eq!(reply_ident(&p, 7), None, "an echo request is not a reply");
    }

    #[test]
    #[ignore = "sends one echo to this Mac's real router; cargo test native_router_ping -- --ignored --nocapture"]
    fn native_router_ping_reaches_the_gateway() {
        let ip = find_gateway().expect("no local default gateway");
        let (outcome, ident) = ping(ip, 1);
        println!("router {ip}: {outcome:?}, sent identifier {IDENT:#06x}, reply identifier {ident:04x?}");
        match outcome {
            Outcome::Reply(rtt) => assert!(rtt > 0.0 && rtt < 1000.0),
            other => panic!("the router did not answer: {other:?}"),
        }
    }
}
