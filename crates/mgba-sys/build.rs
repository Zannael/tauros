//! Builds libmgba statically from the pinned upstream source fetched by
//! `scripts/setup_toolchain.py` (toolchain/sources.json) and generates bindings.
//! The approach (cmake static build + bindgen allowlists) follows ocnc/mgba-rs
//! (MIT), credited in the README; no code is copied from it.
use std::env;
use std::path::PathBuf;

const SOURCE: &str = "sources/mgba-0.10.5";

fn mgba_source() -> PathBuf {
    if let Some(dir) = env::var_os("TAUROS_MGBA_SRC") {
        return PathBuf::from(dir);
    }
    let data = env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))
        .expect("set HOME, TAUROS_DATA or TAUROS_MGBA_SRC");
    data.join(SOURCE)
}

fn main() {
    println!("cargo:rerun-if-env-changed=TAUROS_MGBA_SRC");
    println!("cargo:rerun-if-env-changed=TAUROS_DATA");
    println!("cargo:rerun-if-changed=wrapper.h");
    let src = mgba_source();
    if !src.join("CMakeLists.txt").exists() {
        panic!(
            "pinned mGBA source not found at {}; run `python3 scripts/setup_toolchain.py`",
            src.display()
        );
    }

    let mut config = cmake::Config::new(&src);
    for (key, value) in [
        ("BUILD_STATIC", "ON"),
        ("BUILD_SHARED", "OFF"),
        ("M_CORE_GBA", "ON"),
        ("M_CORE_GB", "ON"),
        ("USE_ZLIB", "ON"),
        ("SKIP_GIT", "ON"),
        ("DISABLE_DEPS", "ON"),
        ("ENABLE_SCRIPTING", "OFF"),
        ("ENABLE_DEBUGGERS", "OFF"),
        ("ENABLE_GDB_STUB", "OFF"),
    ] {
        config.define(key, value);
    }
    for off in [
        "BUILD_QT", "BUILD_SDL", "BUILD_GL", "BUILD_GLES2", "BUILD_GLES3", "BUILD_LIBRETRO",
        // GCC LTO objects in the archive are unreadable to rustc's linker (release builds).
        "BUILD_LTO", "BUILD_PERF", "BUILD_TEST", "BUILD_SUITE", "BUILD_CINEMA", "BUILD_HEADLESS",
        "BUILD_EXAMPLE", "BUILD_PYTHON", "BUILD_DOCGEN", "BUILD_MAINTAINER_TOOLS", "USE_FFMPEG",
        "USE_PNG", "USE_LIBZIP", "USE_SQLITE3", "USE_ELF", "USE_LUA", "USE_JSON_C",
        "USE_FREETYPE", "USE_DISCORD_RPC", "USE_EDITLINE", "USE_LZMA", "USE_MINIZIP", "USE_EPOXY",
    ] {
        config.define(off, "OFF");
    }
    let dst = config.build();

    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=static=mgba");
    for lib in ["z", "m", "pthread"] {
        println!("cargo:rustc-link-lib={lib}");
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", src.join("include").display()))
        .clang_arg(format!("-I{}", dst.join("include").display()))
        .clang_arg("-DM_CORE_GBA")
        .clang_arg("-DM_CORE_GB")
        .clang_arg("-DUSE_ZLIB")
        .clang_arg("-DENABLE_VFS")
        .clang_arg("-DENABLE_VFS_FD")
        .clang_arg("-DENABLE_DIRECTORIES")
        .allowlist_type("mCore")
        .allowlist_type("GBAKey")
        .allowlist_type("VFile")
        .allowlist_function("GBACoreCreate")
        .allowlist_function("mCoreInitConfig")
        .allowlist_function("mCoreConfigDeinit")
        .allowlist_function("mLogSetDefaultLogger")
        .allowlist_function("VFileOpen")
        .allowlist_function("VFileFromMemory")
        .allowlist_type("mCoreMemoryBlock")
        .allowlist_type("mCoreMemoryBlockFlags")
        .allowlist_function("GBAIsROM")
        .allowlist_function("blip_set_rates")
        .allowlist_function("blip_samples_avail")
        .allowlist_function("blip_read_samples")
        .allowlist_function("blip_clear")
        .allowlist_var("projectVersion")
        .allowlist_var("gitCommit")
        .allowlist_var("GBA_VIDEO_HORIZONTAL_PIXELS")
        .allowlist_var("GBA_VIDEO_VERTICAL_PIXELS")
        .derive_default(true)
        .prepend_enum_name(false)
        .layout_tests(false)
        .generate()
        .expect("bindgen failed");
    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("write bindings");
}
