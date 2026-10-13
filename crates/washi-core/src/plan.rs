// SPDX-License-Identifier: GPL-2.0-or-later
use washi_ivec::Request;

/// One step of a flow, as shown by a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A request that is sent to the printer.
    Send(Request),
    /// Reads that follow (progress polling, read-back), described in words.
    Poll(String),
}

/// What a flow would send, in order. Building a plan never touches the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub title: String,
    pub steps: Vec<Step>,
}

impl Plan {
    /// The requests in order (without the polling notes).
    pub fn requests(&self) -> impl Iterator<Item = &Request> {
        self.steps.iter().filter_map(|s| match s {
            Step::Send(r) => Some(r),
            Step::Poll(_) => None,
        })
    }
}
