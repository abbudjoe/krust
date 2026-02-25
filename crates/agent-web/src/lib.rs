//! # krust-agent-web
//!
//! Web interaction abstractions for AI agents. Backend-agnostic.
//!
//! This crate defines:
//! - **Page model**: an agent-friendly representation of a web page
//! - **Action primitives**: navigate, click, type, extract, wait
//! - **Evidence types**: screenshots, element state, page snapshots
//! - **Backend trait**: implement for Playwright, CDP, accessibility, etc.
//!
//! Backends are pluggable:
//! - `CdpBackend` — Chrome DevTools Protocol (desktop/Linux, via chromiumoxide)
//! - `AccessibilityBackend` — platform accessibility APIs (Android/iOS, via FFI)
//! - `NativeBackend` — direct browser control (future Linux mobile OS)

pub mod page;
pub mod action;
pub mod backend;
pub mod evidence;
pub mod cdp;
pub mod tools;
