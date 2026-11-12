#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Probe rustc and LLVM KASAN instrumentation capability for x86_64-unknown-none.
# Follows docs/kasan.md Section 4 ("工具链与编译器探针").
set -euo pipefail

echo "=== Asterinas KASAN Compiler & ABI Probe ==="
echo "Host: $(uname -smr)"
echo "Toolchain:"
rustc -vV

TMPDIR=$(mktemp -d /tmp/kasan_probe.XXXXXX)
trap 'rm -rf "$TMPDIR"' EXIT

cat << 'RUST_SRC' > "$TMPDIR/probe.rs"
#![no_std]
#![no_main]
#![feature(sanitize)]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

// 1. Primitive loads
#[no_mangle]
pub unsafe extern "C" fn probe_load1(p: *const u8) -> u8 { *p }
#[no_mangle]
pub unsafe extern "C" fn probe_load2(p: *const u16) -> u16 { *p }
#[no_mangle]
pub unsafe extern "C" fn probe_load4(p: *const u32) -> u32 { *p }
#[no_mangle]
pub unsafe extern "C" fn probe_load8(p: *const u64) -> u64 { *p }
#[no_mangle]
pub unsafe extern "C" fn probe_load16(p: *const u128) -> u128 { *p }

// 2. Primitive stores
#[no_mangle]
pub unsafe extern "C" fn probe_store1(p: *mut u8, v: u8) { *p = v; }
#[no_mangle]
pub unsafe extern "C" fn probe_store2(p: *mut u16, v: u16) { *p = v; }
#[no_mangle]
pub unsafe extern "C" fn probe_store4(p: *mut u32, v: u32) { *p = v; }
#[no_mangle]
pub unsafe extern "C" fn probe_store8(p: *mut u64, v: u64) { *p = v; }
#[no_mangle]
pub unsafe extern "C" fn probe_store16(p: *mut u128, v: u128) { *p = v; }

// 3. Arbitrary / non-power-of-two size loads and stores
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Struct3([u8; 3]);

#[no_mangle]
pub unsafe extern "C" fn probe_load_n(p: *const Struct3) -> Struct3 { *p }
#[no_mangle]
pub unsafe extern "C" fn probe_store_n(p: *mut Struct3, v: Struct3) { *p = v; }

// 4. Memory intrinsics
#[no_mangle]
pub unsafe extern "C" fn probe_memcpy(dst: *mut u8, src: *const u8, n: usize) {
    core::ptr::copy_nonoverlapping(src, dst, n);
}
#[no_mangle]
pub unsafe extern "C" fn probe_memmove(dst: *mut u8, src: *const u8, n: usize) {
    core::ptr::copy(src, dst, n);
}
#[no_mangle]
pub unsafe extern "C" fn probe_memset(dst: *mut u8, val: u8, n: usize) {
    core::ptr::write_bytes(dst, val, n);
}

// 5. Function exemption via #[sanitize(address = "off")]
#[sanitize(address = "off")]
#[no_mangle]
pub unsafe extern "C" fn probe_exempt(p: *mut u8) { *p = 42; }

// 6. Noreturn handling
#[no_mangle]
pub unsafe extern "C" fn probe_panic() -> ! {
    panic!("kasan probe noreturn");
}
RUST_SRC

echo ""
echo "--- 1. Target Support Probe ---"
if rustc --target x86_64-unknown-none --crate-type=lib -Zsanitizer=address "$TMPDIR/probe.rs" -o "$TMPDIR/asan.o" 2>"$TMPDIR/err_asan.txt"; then
    echo "ERROR: -Zsanitizer=address unexpectedly succeeded on bare-metal"
    exit 1
else
    echo "Confirmed: -Zsanitizer=address rejected on x86_64-unknown-none:"
    head -n 2 "$TMPDIR/err_asan.txt"
fi

echo ""
echo "--- 2. Outline Mode ABI Symbols (-asan-instrumentation-with-call-threshold=0) ---"
rustc --target x86_64-unknown-none --crate-type=lib \
    -Zsanitizer=kernel-address \
    -Cunsafe-allow-abi-mismatch=sanitizer \
    -O \
    -C llvm-args="-asan-instrumentation-with-call-threshold=0" \
    -C llvm-args="-asan-stack=0" \
    -C llvm-args="-asan-globals=0" \
    -C llvm-args="-asan-kernel-mem-intrinsic-prefix=1" \
    "$TMPDIR/probe.rs" -o "$TMPDIR/kasan_outline.o"

nm -u "$TMPDIR/kasan_outline.o" 2>/dev/null | grep -E "__asan" | sort -u

echo ""
echo "--- 3. Inline Mode ABI Symbols ---"
rustc --target x86_64-unknown-none --crate-type=lib \
    -Zsanitizer=kernel-address \
    -Cunsafe-allow-abi-mismatch=sanitizer \
    -O \
    -C llvm-args="-asan-stack=0" \
    -C llvm-args="-asan-globals=0" \
    -C llvm-args="-asan-kernel-mem-intrinsic-prefix=1" \
    "$TMPDIR/probe.rs" -o "$TMPDIR/kasan_inline.o"

nm -u "$TMPDIR/kasan_inline.o" 2>/dev/null | grep -E "__asan" | sort -u

echo ""
echo "--- 4. Function Exemption Check ---"
if objdump -d "$TMPDIR/kasan_outline.o" | grep -A 3 "<probe_exempt>:" | grep -q "__asan"; then
    echo "ERROR: probe_exempt has unexpected ASan instrumentation"
    exit 1
else
    echo "Confirmed: probe_exempt is uninstrumented (pure mov + ret)."
fi

echo ""
echo "=== KASAN Compiler & ABI Probe PASSED ==="
