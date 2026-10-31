//! SyzAster: Coverage-Guided Kernel Fuzzing Prototype Tool for Asterinas Framekernel.
//!
//! This crate provides the core library and CLI utilities for building KCOV-instrumented
//! Asterinas NixOS images, validating KCOV ABI and compiler instrumentation, managing
//! kernel source trees and patch series, running QEMU VMs and reproducers, and evaluating
//! fuzzing campaigns with statistical analysis.

pub mod pkg;
