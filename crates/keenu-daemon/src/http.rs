// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! One HTTP agent for every request keenud makes, so they share its connections and TLS setup.

use std::sync::LazyLock;
use std::time::Duration;

use ureq::typestate::WithoutBody;
use ureq::{Agent, RequestBuilder};

static AGENT: LazyLock<Agent> = LazyLock::new(|| Agent::config_builder().build().into());

/// A GET of `url` that gives up after `timeout` in all. Blocking once called.
pub fn get(url: &str, timeout: Duration) -> RequestBuilder<WithoutBody> {
    AGENT
        .get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
}
