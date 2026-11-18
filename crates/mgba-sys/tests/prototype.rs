//! DEC-002 prototype: libmgba links and its core lifecycle works from Rust.
use std::ffi::{CStr, CString};
use tauros_mgba_sys as sys;

unsafe fn create() -> *mut sys::mCore {
    let core = sys::GBACoreCreate();
    assert!(!core.is_null(), "GBACoreCreate returned null");
    assert!(((*core).init.unwrap())(core), "core init failed");
    sys::mCoreInitConfig(core, std::ptr::null());
    core
}

unsafe fn destroy(core: *mut sys::mCore) {
    sys::mCoreConfigDeinit(&mut (*core).config);
    ((*core).deinit.unwrap())(core);
}

#[test]
fn rom_less_init_and_teardown() {
    unsafe {
        let version = CStr::from_ptr(sys::projectVersion).to_string_lossy();
        assert_eq!(version, "0.10.5");
        let core = create();
        destroy(core);
    }
}

/// Loads the base ROM from `TAUROS_BASE_ROM` (built by setup_toolchain.py), runs
/// frames, reads the cartridge header and takes a savestate. Ignored by default:
/// it needs a locally built ROM that is never part of the repository.
#[test]
#[ignore]
fn base_rom_runs_reads_memory_and_saves_state() {
    let rom = std::env::var("TAUROS_BASE_ROM").expect("set TAUROS_BASE_ROM");
    unsafe {
        let core = create();
        let path = CString::new(rom).unwrap();
        let vf = sys::VFileOpen(path.as_ptr(), libc_o_rdonly());
        assert!(!vf.is_null(), "cannot open ROM");
        assert!(((*core).loadROM.unwrap())(core, vf), "loadROM failed");
        let (mut w, mut h) = (0u32, 0u32);
        ((*core).desiredVideoDimensions.unwrap())(core, &mut w, &mut h);
        let mut video = vec![0u32; (w * h) as usize];
        ((*core).setVideoBuffer.unwrap())(core, video.as_mut_ptr().cast(), w as usize);
        ((*core).reset.unwrap())(core);
        for _ in 0..120 {
            ((*core).runFrame.unwrap())(core);
        }
        assert!(((*core).frameCounter.unwrap())(core) >= 120);
        // Cartridge header game code at 0x080000AC: "BPEE" for this Emerald build.
        let code: Vec<u8> = (0..4)
            .map(|i| ((*core).busRead8.unwrap())(core, 0x0800_00AC + i) as u8)
            .collect();
        assert_eq!(&code, b"BPEE");
        let size = ((*core).stateSize.unwrap())(core);
        assert!(size > 0);
        let mut state = vec![0u8; size];
        assert!(((*core).saveState.unwrap())(core, state.as_mut_ptr().cast()));
        assert!(((*core).loadState.unwrap())(core, state.as_ptr().cast()));
        assert!(video.iter().any(|&p| p != 0), "framebuffer stayed blank");
        destroy(core);
    }
}

fn libc_o_rdonly() -> i32 {
    0 // O_RDONLY on Linux
}
