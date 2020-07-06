extern crate cc;

use std::env::var;

fn main() {
    let target_os = var("CARGO_CFG_TARGET_OS").unwrap();
    let target_family = var("CARGO_CFG_TARGET_FAMILY").unwrap();
    let target_env = var("CARGO_CFG_TARGET_ENV").unwrap();
    let target_triple = var("TARGET").unwrap();

    let mut base_config = cc::Build::new();

    if target_os == "freebsd" {
        let src_base = var("SRC_BASE").unwrap_or("freebsd".to_string());

        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb20.c"));
        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb20_desc.c"));
        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb20_ugen20.c"));

        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb10.c"));
        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb10_desc.c"));
        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb10_hotplug.c"));
        base_config.file(format!("{}{}", src_base, "/lib/libusb/libusb10_io.c"));
        base_config.compile("libusb.a");
        return;
    }

    base_config.include(".");
    base_config.include("libusb/libusb");

    if target_os == "macos" {
        base_config.define("OS_DARWIN", Some("1"));
        base_config.file("libusb/libusb/os/darwin_usb.c");
        link_framework("CoreFoundation");
        link_framework("IOKit");
        link("objc", false);
    }
    if target_os == "linux" {
        base_config.define("OS_LINUX", Some("1"));
        base_config.define("HAVE_ASM_TYPES_H", Some("1"));
        base_config.define("HAVE_LINUX_NETLINK_H", Some("1"));
        base_config.define("HAVE_SYS_SOCKET_H", Some("1"));
        base_config.define("USBI_TIMERFD_AVAILABLE", Some("1"));
        base_config.file("libusb/libusb/os/linux_netlink.c");
        base_config.file("libusb/libusb/os/linux_usbfs.c");
        base_config.define("POLL_NFDS_TYPE", Some("nfds_t"));
        base_config.define("_GNU_SOURCE", Some("1"));
        if target_triple == "armv7-unknown-linux-musleabihf" {
            base_config.flag("-mfpu=neon");
        }
    }

    if target_family == "unix" {
        base_config.define("HAVE_DLFCN_H", Some("1"));
        base_config.define("HAVE_GETTIMEOFDAY", Some("1"));
        base_config.define("HAVE_INTTYPES_H", Some("1"));
        base_config.define("HAVE_MEMORY_H", Some("1"));
        base_config.define("HAVE_POLL_H", Some("1"));
        base_config.define("HAVE_STDINT_H", Some("1"));
        base_config.define("HAVE_STDLIB_H", Some("1"));
        base_config.define("HAVE_STRINGS_H", Some("1"));
        base_config.define("HAVE_STRING_H", Some("1"));
        base_config.define("HAVE_STRUCT_TIMESPEC", Some("1"));
        base_config.define("HAVE_SYS_STAT_H", Some("1"));
        base_config.define("HAVE_SYS_TIME_H", Some("1"));
        base_config.define("HAVE_SYS_TYPES_H", Some("1"));
        base_config.define("HAVE_UNISTD_H", Some("1"));
        base_config.define("POLL_NFDS_TYPE", Some("nfds_t"));
        base_config.define("STDC_HEADERS", Some("1"));
        base_config.define("THREADS_POSIX", Some("1"));
        base_config.define(
            "DEFAULT_VISIBILITY",
            Some("__attribute__((visibility(\"default\")))"),
        );

        base_config.file("libusb/libusb/os/poll_posix.c");
        base_config.file("libusb/libusb/os/threads_posix.c");

        if target_os == "netbsd" {
            base_config.file("libusb/libusb/os/netbsd_usb.c");
        }

        if target_os == "openbsd" {
            base_config.file("libusb/libusb/os/openbsd_usb.c");
        }

        if target_os == "dragonfly" {
            base_config.file("libusb/libusb/os/openbsd_usb.c");
        }
    }

    if target_os == "windows" {
        base_config.define("OS_WINDOWS", Some("1"));
        base_config.file("libusb/libusb/os/poll_windows.c");
        base_config.file("libusb/libusb/os/threads_windows.c");
        base_config.file("libusb/libusb/os/windows_winusb.c");
        base_config.file("libusb/libusb/os/windows_nt_common.c");
        base_config.file("libusb/libusb/os/windows_usbdk.c");

        base_config.define("DEFAULT_VISIBILITY", Some(""));
        if !target_env.contains("gnu") {
            base_config.define("_TIMESPEC_DEFINED", Some("1"));
        }
        base_config.define("POLL_NFDS_TYPE", Some("unsigned int"));
        base_config.define("HAVE_SIGNAL_H", Some("1"));
        base_config.define("HAVE_SYS_TYPES_H", Some("1"));
        link("user32", false);
    }

    base_config.file("libusb/libusb/core.c");
    base_config.file("libusb/libusb/descriptor.c");
    base_config.file("libusb/libusb/hotplug.c");
    base_config.file("libusb/libusb/io.c");
    base_config.file("libusb/libusb/strerror.c");
    base_config.file("libusb/libusb/sync.c");

    base_config.compile("libusb.a");
}

pub fn link(name: &str, bundled: bool) {
    let target = var("TARGET").unwrap();
    let target: Vec<_> = target.split('-').collect();
    if target.get(2) == Some(&"windows") {
        println!("cargo:rustc-link-lib=dylib={}", name);
        if bundled && target.get(3) == Some(&"gnu") {
            let dir = var("CARGO_MANIFEST_DIR").unwrap();
            println!("cargo:rustc-link-search=native={}/{}", dir, target[0]);
        }
    } else {
        println!("cargo:rustc-link-lib=dylib={}", name);
    }
}

pub fn link_framework(name: &str) {
    println!("cargo:rustc-link-lib=framework={}", name);
}
