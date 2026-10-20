//! Raw FFI bindings: Mach host APIs, libproc, sysctl, ifaddrs, IOKit,
//! CoreAudio (AudioObject properties), and SystemConfiguration.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_double, c_int, c_long, c_uint, c_ulong, c_void};

pub type kern_return_t = c_int;
pub type mach_port_t = c_uint;
pub type natural_t = c_uint;
pub type io_object_t = c_uint;
pub type io_iterator_t = c_uint;
pub type io_registry_entry_t = c_uint;
pub type boolean_t = c_int;
pub type CFTypeRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFMutableDictionaryRef = *mut c_void;
pub type CFArrayRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFTypeID = c_ulong;
pub type CFIndex = c_long;
pub type Boolean = u8;
pub type OSStatus = i32;
pub type AudioObjectID = u32;
pub type AudioObjectPropertySelector = u32;
pub type AudioObjectPropertyScope = u32;
pub type AudioObjectPropertyElement = u32;
pub type Float32 = f32;
pub type UInt32 = u32;

pub const KERN_SUCCESS: kern_return_t = 0;
pub const PROC_ALL_PIDS: u32 = 1;
pub const PROC_PIDTBSDINFO: c_int = 3;
pub const PROC_PIDVNODEPATHINFO: c_int = 9;
pub const PROC_PIDFDSOCKETINFO: c_int = 3;
pub const PROC_PIDLISTFDS: c_int = 1;
pub const PROX_FDTYPE_SOCKET: u32 = 2;
pub const SOCKINFO_IN: i32 = 1;
pub const SOCKINFO_TCP: i32 = 2;
pub const TSI_S_LISTEN: i32 = 1;
pub const RUSAGE_INFO_V4: c_int = 4;
pub const RUSAGE_INFO_V6: c_int = 6;
pub const PROCESSOR_CPU_LOAD_INFO: c_int = 2;
pub const HOST_VM_INFO64: c_int = 4;
pub const CPU_STATE_USER: usize = 0;
pub const CPU_STATE_SYSTEM: usize = 1;
pub const CPU_STATE_IDLE: usize = 2;
pub const CPU_STATE_NICE: usize = 3;

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct mach_timebase_info_data_t {
    pub numer: u32,
    pub denom: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct proc_bsdinfo {
    pub pbi_flags: u32,
    pub pbi_status: u32,
    pub pbi_xstatus: u32,
    pub pbi_pid: u32,
    pub pbi_ppid: u32,
    pub pbi_uid: u32,
    pub pbi_gid: u32,
    pub pbi_ruid: u32,
    pub pbi_rgid: u32,
    pub pbi_svuid: u32,
    pub pbi_svgid: u32,
    pub rfu_1: u32,
    pub pbi_comm: [c_char; 16],
    pub pbi_name: [c_char; 32],
    pub pbi_nfiles: u32,
    pub pbi_pgid: u32,
    pub pbi_pjobc: u32,
    pub e_tdev: u32,
    pub e_tpgid: u32,
    pub pbi_nice: i32,
    pub pbi_start_tvsec: u64,
    pub pbi_start_tvusec: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct vnode_info {
    pub vi_stat: [u64; 17], // 136-byte, 8-byte-aligned struct vinfo_stat
    pub vi_type: c_int,
    pub vi_pad: c_int,
    pub vi_fsid: [u32; 2], // fsid_t
}

#[repr(C)]
pub struct _unused_proc_vnodepathinfo {
    pub pvi_cdir: vnode_info,
    pub pvi_rdir: vnode_info,
}
// vnode_info is followed by vi_path[MAXPATHLEN=1024]; define with path inline:
#[repr(C)]
pub struct vnode_info_path {
    pub vi_stat: [u64; 17], // 136-byte, 8-byte-aligned struct vinfo_stat
    pub vi_type: c_int,
    pub vi_pad: c_int,
    pub vi_fsid: [u32; 2],
    pub vi_path: [c_char; 1024],
}
#[repr(C)]
pub struct proc_vnodepathinfo_full {
    pub pvi_cdir: vnode_info_path,
    pub pvi_rdir: vnode_info_path,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct rusage_info_v4 {
    pub ri_uuid: [u8; 16],
    pub ri_user_time: u64,
    pub ri_system_time: u64,
    pub ri_pkg_idle_wkups: u64,
    pub ri_interrupt_wkups: u64,
    pub ri_pageins: u64,
    pub ri_wired_size: u64,
    pub ri_resident_size: u64,
    pub ri_phys_footprint: u64,
    pub ri_proc_start_abstime: u64,
    pub ri_proc_exit_abstime: u64,
    pub ri_child_user_time: u64,
    pub ri_child_system_time: u64,
    pub ri_child_pkg_idle_wkups: u64,
    pub ri_child_interrupt_wkups: u64,
    pub ri_child_pageins: u64,
    pub ri_child_elapsed_abstime: u64,
    pub ri_diskio_bytesread: u64,
    pub ri_diskio_byteswritten: u64,
    pub ri_cpu_time_qos_default: u64,
    pub ri_cpu_time_qos_maintenance: u64,
    pub ri_cpu_time_qos_background: u64,
    pub ri_cpu_time_qos_utility: u64,
    pub ri_cpu_time_qos_legacy: u64,
    pub ri_cpu_time_qos_user_initiated: u64,
    pub ri_cpu_time_qos_user_interactive: u64,
    pub ri_billed_system_time: u64,
    pub ri_serviced_system_time: u64,
    pub ri_logical_writes: u64,
    pub ri_lifetime_max_phys_footprint: u64,
    pub ri_instructions: u64,
    pub ri_cycles: u64,
    pub ri_billed_energy: u64,
    pub ri_serviced_energy: u64,
    pub ri_interval_max_phys_footprint: u64,
    pub ri_runnable_time: u64,
    // Version 6 fields; a version 4 read leaves them zero.
    pub ri_flags: u64,
    pub ri_user_ptime: u64,
    pub ri_system_ptime: u64,
    pub ri_pinstructions: u64,
    pub ri_pcycles: u64,
    /// Energy macOS accounts to the process, in nanojoules (Apple silicon).
    pub ri_energy_nj: u64,
    pub ri_penergy_nj: u64,
    pub ri_v6_rest: [u64; 14],
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct vm_statistics64 {
    pub free_count: natural_t,
    pub active_count: natural_t,
    pub inactive_count: natural_t,
    pub wire_count: natural_t,
    pub zero_fill_count: u64,
    pub reactivations: u64,
    pub pageins: u64,
    pub pageouts: u64,
    pub faults: u64,
    pub cow_faults: u64,
    pub lookups: u64,
    pub hits: u64,
    pub purges: u64,
    pub purgeable_count: natural_t,
    pub speculative_count: natural_t,
    pub decompressions: u64,
    pub compressions: u64,
    pub swapins: u64,
    pub swapouts: u64,
    pub compressor_page_count: natural_t,
    pub throttled_count: natural_t,
    pub external_page_count: natural_t,
    pub internal_page_count: natural_t,
    pub total_uncompressed_pages_in_compressor: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct xsw_usage {
    pub xsu_total: u64,
    pub xsu_avail: u64,
    pub xsu_used: u64,
    pub xsu_pagesize: u32,
    pub xsu_encrypted: boolean_t,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct sockaddr {
    pub sa_len: u8,
    pub sa_family: u8,
    pub sa_data: [c_char; 14],
}

#[repr(C)]
pub struct if_data {
    pub ifi_type: u8,
    pub ifi_typelen: u8,
    pub ifi_physical: u8,
    pub ifi_addrlen: u8,
    pub ifi_hdrlen: u8,
    pub ifi_recvquota: u8,
    pub ifi_xmitquota: u8,
    pub ifi_unused1: u8,
    pub ifi_mtu: u32,
    pub ifi_metric: u32,
    pub ifi_baudrate: u32,
    pub ifi_ipackets: u32,
    pub ifi_ierrors: u32,
    pub ifi_opackets: u32,
    pub ifi_oerrors: u32,
    pub ifi_collisions: u32,
    pub ifi_ibytes: u32,
    pub ifi_obytes: u32,
    pub ifi_imcasts: u32,
    pub ifi_omcasts: u32,
    pub ifi_iqdrops: u32,
    pub ifi_noproto: u32,
    pub ifi_recvtiming: u32,
    pub ifi_xmittiming: u32,
    pub ifi_lastchange: [c_long; 2],
}

#[repr(C)]
pub struct ifaddrs {
    pub ifa_next: *mut ifaddrs,
    pub ifa_name: *mut c_char,
    pub ifa_flags: c_uint,
    pub ifa_addr: *mut sockaddr,
    pub ifa_netmask: *mut sockaddr,
    pub ifa_dstaddr: *mut sockaddr,
    pub ifa_data: *mut if_data,
}
pub const AF_LINK: u8 = 18;
pub const AF_INET: u8 = 2;
pub const AF_INET6: u8 = 30;
pub const IFF_LOOPBACK: c_uint = 0x8;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioObjectPropertyAddress {
    pub mSelector: AudioObjectPropertySelector,
    pub mScope: AudioObjectPropertyScope,
    pub mElement: AudioObjectPropertyElement,
}

pub const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
pub const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
pub const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
pub const K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT: u32 = u32::from_be_bytes(*b"inpt");
pub const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;
pub const K_AUDIO_HARDWARE_PROPERTY_DEVICES: u32 = u32::from_be_bytes(*b"dev#");
pub const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = u32::from_be_bytes(*b"dOut");
pub const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE: u32 = u32::from_be_bytes(*b"dIn ");
pub const K_AUDIO_DEVICE_PROPERTY_DEVICE_CAN_BE_DEFAULT_DEVICE: u32 = u32::from_be_bytes(*b"dflt");
pub const K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST: u32 = u32::from_be_bytes(*b"prs#");
pub const K_AUDIO_HARDWARE_PROPERTY_PROCESS_INPUT_MUTE: u32 = u32::from_be_bytes(*b"pmin");
pub const K_AUDIO_HARDWARE_PROPERTY_PROCESS_IS_AUDIBLE: u32 = u32::from_be_bytes(*b"pmut");
pub const K_AUDIO_DEVICE_PROPERTY_DEVICE_NAME_CFSTRING: u32 = u32::from_be_bytes(*b"lnam");
pub const K_AUDIO_DEVICE_PROPERTY_UID: u32 = u32::from_be_bytes(*b"uid ");
pub const K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR: u32 = u32::from_be_bytes(*b"volm");
pub const K_AUDIO_DEVICE_PROPERTY_MUTE: u32 = u32::from_be_bytes(*b"mute");
pub const K_AUDIO_DEVICE_PROPERTY_PROCESS_MUTE: u32 = u32::from_be_bytes(*b"appm");
pub const K_AUDIO_DEVICE_PROPERTY_STREAMS: u32 = u32::from_be_bytes(*b"stm#");
pub const K_AUDIO_PROCESS_PROPERTY_PID: u32 = u32::from_be_bytes(*b"ppid");
pub const K_AUDIO_PROCESS_PROPERTY_BUNDLE_ID: u32 = u32::from_be_bytes(*b"pbid");
pub const K_AUDIO_PROCESS_PROPERTY_IS_RUNNING: u32 = u32::from_be_bytes(*b"pir?");
pub const K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_OUTPUT: u32 = u32::from_be_bytes(*b"piro");
pub const K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_INPUT: u32 = u32::from_be_bytes(*b"piri");

#[link(name = "System", kind = "dylib")]
#[link(name = "IOKit", kind = "framework")]
#[link(name = "CoreAudio", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn mach_host_self() -> mach_port_t;
    pub fn mach_timebase_info(info: *mut mach_timebase_info_data_t) -> kern_return_t;
    pub fn host_processor_info(
        host: mach_port_t,
        flavor: c_int,
        out_count: *mut natural_t,
        out_info: *mut *mut c_int,
        out_count2: *mut natural_t,
    ) -> kern_return_t;
    pub fn host_statistics64(
        host: mach_port_t,
        flavor: c_int,
        out_info: *mut vm_statistics64,
        out_count: *mut natural_t,
    ) -> kern_return_t;
    pub fn host_page_size(host: mach_port_t, out: *mut natural_t) -> kern_return_t;
    pub fn vm_deallocate(target: mach_port_t, addr: usize, size: usize) -> kern_return_t;
    pub fn mach_task_self() -> mach_port_t;

    pub fn proc_listpids(kind: c_uint, typeinfo: c_uint, buffer: *mut c_void, size: c_int)
        -> c_int;
    pub fn proc_pidinfo(
        pid: c_int,
        flavor: c_int,
        arg: u64,
        buffer: *mut c_void,
        size: c_int,
    ) -> c_int;
    pub fn proc_pidfdinfo(
        pid: c_int,
        fd: c_int,
        flavor: c_int,
        buffer: *mut c_void,
        size: c_int,
    ) -> c_int;
    pub fn proc_pidpath(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
    pub fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut rusage_info_v4) -> c_int;

    pub fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
    pub fn getloadavg(loadavg: *mut c_double, nelem: c_int) -> c_int;
    pub fn getifaddrs(ifap: *mut *mut ifaddrs) -> c_int;
    pub fn freeifaddrs(ifa: *mut ifaddrs);

    // IOKit / IOPS
    pub fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    pub fn IOServiceGetMatchingServices(
        main_port: mach_port_t,
        matching: CFMutableDictionaryRef,
        existing: *mut io_iterator_t,
    ) -> kern_return_t;
    pub fn IOIteratorNext(iterator: io_iterator_t) -> io_object_t;
    pub fn IOObjectRelease(object: io_object_t) -> kern_return_t;
    pub fn IORegistryEntryCreateCFProperty(
        entry: io_registry_entry_t,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    pub fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    pub fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    pub fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
    pub fn IOPMAssertionCreateWithName(
        assertion_type: CFStringRef,
        assertion_level: u32,
        assertion_name: CFStringRef,
        assertion_id: *mut u32,
    ) -> kern_return_t;
    pub fn IOPMAssertionRelease(assertion_id: u32) -> kern_return_t;

    // CoreAudio
    pub fn AudioObjectGetPropertyData(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> OSStatus;
    pub fn AudioObjectGetPropertyDataSize(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
    ) -> OSStatus;
    pub fn AudioObjectSetPropertyData(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier_data: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> OSStatus;
    pub fn AudioObjectHasProperty(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
    ) -> Boolean;
    pub fn AudioObjectIsPropertySettable(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        out_is_settable: *mut Boolean,
    ) -> OSStatus;

    // CoreFoundation minimal surface
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFArrayGetCount(arr: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    pub fn CFNumberGetValue(
        number: CFNumberRef,
        the_type: c_int,
        value_ptr: *mut c_void,
    ) -> Boolean;
    pub fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut c_char,
        size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    pub fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        c_string: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFNumberGetTypeID() -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFDictionaryGetTypeID() -> CFTypeID;
    pub fn CFBooleanGetTypeID() -> CFTypeID;
    pub fn CFBooleanGetValue(b: CFTypeRef) -> Boolean;
}

pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;
pub const K_CF_NUMBER_SINT64: c_int = 4;
pub const K_CF_NUMBER_FLOAT64: c_int = 13;
pub const K_CF_NUMBER_SINT32: c_int = 3;

/// Four-char code helper.
pub const fn fcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// `struct proc_fdinfo` from <sys/proc_info.h>: one entry per open descriptor.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct proc_fdinfo {
    pub proc_fd: i32,
    pub proc_fdtype: u32,
}

/// `struct socket_fdinfo` (792 bytes), kept as raw bytes: only a few fields
/// are read, at the offsets below, checked against the macOS SDK in tests.
#[repr(C, align(8))]
pub struct socket_fdinfo {
    pub bytes: [u8; SOCKET_FDINFO_SIZE],
}

pub const SOCKET_FDINFO_SIZE: usize = 792;
/// `soi_family` holds AF_INET (2) or AF_INET6 (30).
const SOI_FAMILY: usize = 24 + 160;
const SOI_KIND: usize = 24 + 232;
const TCP_LOCAL_PORT: usize = 24 + 240 + 4;
const TCP_STATE: usize = 24 + 240 + 80;

impl socket_fdinfo {
    pub fn zeroed() -> Self {
        Self {
            bytes: [0; SOCKET_FDINFO_SIZE],
        }
    }

    fn int(&self, at: usize) -> i32 {
        i32::from_ne_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }

    /// The local port when this is a TCP socket in the LISTEN state over IPv4
    /// or IPv6. The kernel stores the port in network byte order.
    pub fn listening_tcp_port(&self) -> Option<u16> {
        if self.int(SOI_KIND) != SOCKINFO_TCP
            || self.int(TCP_STATE) != TSI_S_LISTEN
            || !matches!(self.int(SOI_FAMILY), 2 | 30)
        {
            return None;
        }
        let port = u16::from_be(self.int(TCP_LOCAL_PORT) as u16);
        (port != 0).then_some(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_fdinfo_offsets_match_macos_sdk() {
        // From <sys/proc_info.h>: sizeof(socket_fdinfo) 792, psi at 24,
        // soi_family 160, soi_kind 232, soi_proto 240, insi_lport 4 and
        // tcpsi_state 80 inside it.
        assert_eq!(SOCKET_FDINFO_SIZE, 792);
        assert_eq!(std::mem::size_of::<proc_fdinfo>(), 8);
        assert_eq!((SOI_FAMILY, SOI_KIND), (184, 256));
        assert_eq!((TCP_LOCAL_PORT, TCP_STATE), (268, 344));
    }

    fn socket(kind: i32, state: i32, family: i32, port_net: u16) -> socket_fdinfo {
        let mut s = socket_fdinfo::zeroed();
        let mut put = |at: usize, v: i32| s.bytes[at..at + 4].copy_from_slice(&v.to_ne_bytes());
        put(SOI_KIND, kind);
        put(TCP_STATE, state);
        put(SOI_FAMILY, family);
        put(TCP_LOCAL_PORT, port_net as i32);
        s
    }

    #[test]
    fn listening_port_is_converted_from_network_order() {
        let s = socket(SOCKINFO_TCP, TSI_S_LISTEN, 2, 3000u16.to_be());
        assert_eq!(s.listening_tcp_port(), Some(3000));
        let s = socket(SOCKINFO_TCP, TSI_S_LISTEN, 30, 80u16.to_be());
        assert_eq!(s.listening_tcp_port(), Some(80));
    }

    #[test]
    fn only_listening_tcp_sockets_count() {
        let net = 8080u16.to_be();
        assert_eq!(
            socket(SOCKINFO_IN, TSI_S_LISTEN, 2, net).listening_tcp_port(),
            None
        );
        assert_eq!(socket(SOCKINFO_TCP, 4, 2, net).listening_tcp_port(), None);
        assert_eq!(
            socket(SOCKINFO_TCP, TSI_S_LISTEN, 1, net).listening_tcp_port(),
            None
        );
        assert_eq!(
            socket(SOCKINFO_TCP, TSI_S_LISTEN, 2, 0).listening_tcp_port(),
            None
        );
    }

    #[test]
    fn vnode_path_layout_matches_macos_sdk() {
        assert_eq!(std::mem::offset_of!(vnode_info_path, vi_path), 152);
        assert_eq!(std::mem::size_of::<vnode_info_path>(), 1176);
        assert_eq!(std::mem::size_of::<proc_vnodepathinfo_full>(), 2352);
        assert_eq!(std::mem::align_of::<proc_vnodepathinfo_full>(), 8);
    }

    #[test]
    fn rusage_wakeup_layout_matches_macos_sdk() {
        // Version 6 layout from <sys/resource.h>: 464 bytes, energy at 336.
        assert_eq!(std::mem::size_of::<rusage_info_v4>(), 464);
        assert_eq!(std::mem::offset_of!(rusage_info_v4, ri_energy_nj), 336);
        assert_eq!(std::mem::offset_of!(rusage_info_v4, ri_phys_footprint), 72);
        assert_eq!(std::mem::offset_of!(rusage_info_v4, ri_pkg_idle_wkups), 32);
        assert_eq!(std::mem::offset_of!(rusage_info_v4, ri_interrupt_wkups), 40);
    }
}
