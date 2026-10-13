// SPDX-License-Identifier: GPL-2.0-or-later
//! `washid`: the Washi user daemon. Polls the active printer, keeps the snapshot cache fresh,
//! serves the `io.github.Jenari_Dev.Washi` D-Bus API, runs maintenance jobs on a worker and sends
//! desktop notifications.

pub mod dbus;
pub mod messages;
pub mod notify;
pub mod service;
pub mod state;
