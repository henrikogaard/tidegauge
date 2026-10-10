//! Read-only AppleSMC telemetry. The worker owns the connection and key cache.

use crate::ffi::*;
use crate::locale::text as l;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Reading {
    pub key: String,
    pub value: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum SourceState {
    #[default]
    Waiting,
    Ready,
    NotExposed,
    Denied,
    Failed(i32),
}

impl SourceState {
    pub fn label(self) -> String {
        match self {
            Self::Waiting => l("Waiting for sensors", "Venter på sensorer").into(),
            Self::Ready => l("Read-only · AppleSMC", "Kun avlesning · AppleSMC").into(),
            Self::NotExposed => l(
                "AppleSMC is not exposed on this Mac",
                "AppleSMC er ikke tilgjengelig på denne Macen",
            )
            .into(),
            Self::Denied => l(
                "Sensor access denied by macOS",
                "macOS avviste sensortilgang",
            )
            .into(),
            Self::Failed(code) => format!(
                "{} (0x{:08x})",
                l("Sensor access unavailable", "Sensortilgang utilgjengelig"),
                code as u32
            ),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SensorSnapshot {
    pub source: SourceState,
    pub temperatures: Vec<Reading>,
    pub fans: Vec<Reading>,
    pub fan_count: Option<u8>,
    pub collected_at: Option<Instant>,
    pub discovering: bool,
    pub inventory_limited: bool,
}

impl SensorSnapshot {
    pub fn is_fresh(&self) -> bool {
        self.collected_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
    }
}

static LATEST: OnceLock<Arc<Mutex<SensorSnapshot>>> = OnceLock::new();

pub fn latest() -> SensorSnapshot {
    LATEST
        .get()
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_default()
}

pub fn start() {
    LATEST.get_or_init(|| {
        let state = Arc::new(Mutex::new(SensorSnapshot::default()));
        let out = state.clone();
        std::thread::Builder::new()
            .name("tidegauge-sensors".into())
            .spawn(move || loop {
                match Smc::open() {
                    Ok(mut smc) => {
                        let opened = Instant::now();
                        while opened.elapsed() < Duration::from_secs(60) {
                            let sample = smc.sample();
                            *out.lock().unwrap() = sample;
                            std::thread::sleep(Duration::from_secs(2));
                        }
                    }
                    Err(source) => {
                        *out.lock().unwrap() = SensorSnapshot {
                            source,
                            collected_at: Some(Instant::now()),
                            ..Default::default()
                        };
                        std::thread::sleep(Duration::from_secs(30));
                    }
                }
            })
            .expect("sensor worker");
        state
    });
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyInfo {
    size: u32,
    data_type: u32,
    attributes: u8,
}

#[repr(C)]
#[derive(Default)]
struct SmcData {
    key: u32,
    version: [u8; 6],
    // C padding aligns the power-limit block at offset 12.
    power_limit: [u32; 4],
    info: KeyInfo,
    result: u8,
    status: u8,
    command: u8,
    data: u32,
    bytes: [u8; 32],
}

#[repr(u8)]
enum ReadCommand {
    Value = 5,
    KeyAtIndex = 8,
    KeyInfo = 9,
}

extern "C" {
    fn IOServiceGetMatchingService(port: u32, matching: CFMutableDictionaryRef) -> u32;
    fn IOServiceOpen(service: u32, task: u32, kind: u32, connection: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
}

struct Smc {
    connection: u32,
    info: HashMap<u32, Option<KeyInfo>>,
    temperature_keys: Vec<u32>,
    next_key: u32,
    key_count: Option<u32>,
    inventory_limited: bool,
}

impl Drop for Smc {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.connection);
        }
    }
}

impl Smc {
    fn open() -> Result<Self, SourceState> {
        unsafe {
            let service = IOServiceGetMatchingService(0, IOServiceMatching(c"AppleSMC".as_ptr()));
            if service == 0 {
                return Err(SourceState::NotExposed);
            }
            let mut connection = 0;
            let result = IOServiceOpen(service, mach_task_self(), 0, &mut connection);
            IOObjectRelease(service);
            if result != 0 {
                return Err(match result as u32 {
                    0xe00002c1 | 0xe00002e2 => SourceState::Denied,
                    _ => SourceState::Failed(result),
                });
            }
            Ok(Self {
                connection,
                info: HashMap::new(),
                temperature_keys: Vec::new(),
                next_key: 0,
                key_count: None,
                inventory_limited: false,
            })
        }
    }

    fn call(&self, key: u32, command: ReadCommand, index: u32, info: KeyInfo) -> Option<SmcData> {
        let input = SmcData {
            key,
            command: command as u8,
            data: index,
            info,
            ..Default::default()
        };
        let mut output = SmcData::default();
        let mut size = size_of::<SmcData>();
        let result = unsafe {
            IOConnectCallStructMethod(
                self.connection,
                2,
                &input as *const _ as *const c_void,
                size_of::<SmcData>(),
                &mut output as *mut _ as *mut c_void,
                &mut size,
            )
        };
        (result == 0 && size == size_of::<SmcData>() && output.result == 0).then_some(output)
    }

    fn key_info(&mut self, key: u32) -> Option<KeyInfo> {
        if let Some(info) = self.info.get(&key) {
            return *info;
        }
        let info = self
            .call(key, ReadCommand::KeyInfo, 0, KeyInfo::default())
            .map(|out| out.info)
            .filter(|info| (1..=32).contains(&info.size));
        self.info.insert(key, info);
        info
    }

    fn read(&mut self, key: u32) -> Option<(KeyInfo, [u8; 32])> {
        let info = self.key_info(key)?;
        self.call(key, ReadCommand::Value, 0, info)
            .map(|out| (info, out.bytes))
    }

    fn discover(&mut self) {
        if self.key_count.is_none() {
            self.key_count = self.read(fcc(b"#KEY")).and_then(|(info, bytes)| {
                (info.data_type == fcc(b"ui32") && info.size == 4)
                    .then(|| u32::from_be_bytes(bytes[..4].try_into().unwrap()))
            });
        }
        let Some(total) = self.key_count else {
            return;
        };
        self.inventory_limited |= total > 8192;
        let started = Instant::now();
        for _ in 0..256 {
            if self.next_key >= total.min(8192) || started.elapsed() > Duration::from_millis(200) {
                break;
            }
            let Some(out) = self.call(
                0,
                ReadCommand::KeyAtIndex,
                self.next_key,
                KeyInfo::default(),
            ) else {
                break;
            };
            self.next_key += 1;
            let key = out.key.to_be_bytes();
            if key[0] != b'T' || !key.iter().all(u8::is_ascii_graphic) {
                continue;
            }
            if let Some(info) = self.key_info(out.key) {
                if matches!(
                    (info.data_type.to_be_bytes(), info.size),
                    ([b's', b'p', b'7', b'8'], 2) | ([b'f', b'l', b't', b' '], 4)
                ) {
                    if self.temperature_keys.len() < 128 {
                        self.temperature_keys.push(out.key);
                    } else {
                        self.inventory_limited = true;
                    }
                }
            }
        }
        self.temperature_keys.sort_unstable();
    }

    fn sample(&mut self) -> SensorSnapshot {
        self.discover();
        let temperatures = self
            .temperature_keys
            .clone()
            .into_iter()
            .map(|key| Reading {
                key: String::from_utf8_lossy(&key.to_be_bytes()).into_owned(),
                value: self
                    .read(key)
                    .and_then(|(info, bytes)| decode(info, &bytes))
                    .filter(|v| valid_temperature(*v)),
            })
            .collect();
        let fan_count = self.read(fcc(b"FNum")).and_then(|(info, bytes)| {
            (info.data_type == fcc(b"ui8 ") && info.size == 1 && bytes[0] <= 16).then_some(bytes[0])
        });
        let fans = (0..fan_count.unwrap_or(0))
            .map(|index| {
                let key = [b'F', b"0123456789ABCDEF"[index as usize], b'A', b'c'];
                Reading {
                    key: String::from_utf8_lossy(&key).into_owned(),
                    value: self
                        .read(fcc(&key))
                        .and_then(|(info, bytes)| decode(info, &bytes))
                        .filter(|v| (0.0..=30_000.0).contains(v)),
                }
            })
            .collect();
        SensorSnapshot {
            source: SourceState::Ready,
            temperatures,
            fans,
            fan_count,
            collected_at: Some(Instant::now()),
            discovering: self
                .key_count
                .is_some_and(|count| self.next_key < count.min(8192)),
            inventory_limited: self.inventory_limited,
        }
    }
}

fn valid_temperature(value: f64) -> bool {
    // Zero is also used by idle/offline channels; do not present it as measured.
    value.is_finite() && (-40.0..=150.0).contains(&value) && value != 0.0
}

fn decode(info: KeyInfo, bytes: &[u8; 32]) -> Option<f64> {
    let value = match (info.data_type.to_be_bytes(), info.size) {
        ([b's', b'p', b'7', b'8'], 2) => i16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 256.0,
        ([b'f', b'p', b'e', b'2'], 2) => u16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 4.0,
        ([b'f', b'l', b't', b' '], 4) => f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

pub fn preview() -> Option<SensorSnapshot> {
    #[cfg(debug_assertions)]
    if std::env::var("TIDEGAUGE_SENSOR_PREVIEW").as_deref() == Ok("1") {
        return Some(SensorSnapshot {
            source: SourceState::Ready,
            temperatures: vec![
                Reading {
                    key: "TC0P".into(),
                    value: Some(54.5),
                },
                Reading {
                    key: "TG0P".into(),
                    value: Some(48.0),
                },
                Reading {
                    key: "Tp0X".into(),
                    value: None,
                },
            ],
            fans: vec![
                Reading {
                    key: "F0Ac".into(),
                    value: Some(1200.0),
                },
                Reading {
                    key: "F1Ac".into(),
                    value: Some(0.0),
                },
            ],
            fan_count: Some(2),
            collected_at: Some(Instant::now()),
            ..Default::default()
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smc_layout_matches_read_protocol() {
        assert_eq!(size_of::<SmcData>(), 80);
        assert_eq!(std::mem::offset_of!(SmcData, power_limit), 12);
        assert_eq!(std::mem::offset_of!(SmcData, info), 28);
        assert_eq!(std::mem::offset_of!(SmcData, result), 40);
        assert_eq!(std::mem::offset_of!(SmcData, command), 42);
        assert_eq!(std::mem::offset_of!(SmcData, data), 44);
        assert_eq!(std::mem::offset_of!(SmcData, bytes), 48);
    }

    #[test]
    fn decode_native_temperature_and_fan_units() {
        let mut bytes = [0; 32];
        bytes[..2].copy_from_slice(&[0x2a, 0x80]);
        let mut info = KeyInfo {
            size: 2,
            data_type: fcc(b"sp78"),
            attributes: 0,
        };
        assert_eq!(decode(info, &bytes), Some(42.5));
        bytes[..2].copy_from_slice(&(-256i16).to_be_bytes());
        assert_eq!(decode(info, &bytes), Some(-1.0));
        info.data_type = fcc(b"fpe2");
        bytes[..2].copy_from_slice(&8000u16.to_be_bytes());
        assert_eq!(decode(info, &bytes), Some(2000.0));
        info = KeyInfo {
            size: 4,
            data_type: fcc(b"flt "),
            attributes: 0,
        };
        bytes[..4].copy_from_slice(&2400f32.to_le_bytes());
        assert_eq!(decode(info, &bytes), Some(2400.0));
        bytes[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(decode(info, &bytes), None);
        info.size = 2;
        assert_eq!(decode(info, &bytes), None);
        assert!(!valid_temperature(0.0));
        assert!(!valid_temperature(999.0));
    }

    #[test]
    fn missing_and_stale_samples_are_not_fresh() {
        let mut sample = SensorSnapshot::default();
        assert!(!sample.is_fresh());
        sample.collected_at = Some(Instant::now());
        assert!(sample.is_fresh());
        sample.collected_at = Some(Instant::now() - Duration::from_secs(11));
        assert!(!sample.is_fresh());
    }

    #[test]
    #[ignore = "Reads this Mac's sensors; physical hardware comparison required"]
    fn native_sensor_probe() {
        match Smc::open() {
            Ok(mut smc) => {
                let snapshot = smc.sample();
                println!("{snapshot:#?}");
                assert!(snapshot
                    .temperatures
                    .iter()
                    .all(|r| r.value.is_none_or(valid_temperature)));
            }
            Err(source) => println!("Sensor source: {source:?}"),
        }
    }
}
