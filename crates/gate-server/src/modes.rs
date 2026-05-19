//! Runtime mode routing for `gate-server`.
//!
//! `all` keeps the historical single-binary behavior. `gateway` and
//! `controlplane` physically separate hot data-plane routes from admin/control
//! routes while keeping the deployment artifact unchanged. `worker` does not
//! build an HTTP router; the binary runs background jobs only.

use crate::{AppState, build_controlplane_router, build_gateway_router, build_router};
use axum::Router;
use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    All,
    Gateway,
    ControlPlane,
    Worker,
}

impl RuntimeMode {
    pub fn from_env() -> Result<Self, String> {
        match std::env::var("KOOIX_MODE") {
            Ok(raw) => raw.parse(),
            Err(std::env::VarError::NotPresent) => Ok(Self::All),
            Err(e) => Err(format!("failed to read KOOIX_MODE: {e}")),
        }
    }

    pub fn serves_http(self) -> bool {
        !matches!(self, Self::Worker)
    }

    pub fn runs_workers(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Gateway => "gateway",
            Self::ControlPlane => "controlplane",
            Self::Worker => "worker",
        }
    }
}

impl FromStr for RuntimeMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "all" => Ok(Self::All),
            "gateway" | "data-plane" | "dataplane" => Ok(Self::Gateway),
            "controlplane" | "control-plane" | "control" => Ok(Self::ControlPlane),
            "worker" | "workers" => Ok(Self::Worker),
            other => Err(format!(
                "invalid KOOIX_MODE={other}; expected all|gateway|controlplane|worker"
            )),
        }
    }
}

pub fn build_router_for_mode(mode: RuntimeMode, state: AppState) -> Option<Router> {
    match mode {
        RuntimeMode::All => Some(build_router(state)),
        RuntimeMode::Gateway => Some(build_gateway_router(state)),
        RuntimeMode::ControlPlane => Some(build_controlplane_router(state)),
        RuntimeMode::Worker => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert_eq!("all".parse::<RuntimeMode>().unwrap(), RuntimeMode::All);
        assert_eq!(
            "gateway".parse::<RuntimeMode>().unwrap(),
            RuntimeMode::Gateway
        );
        assert_eq!(
            "data-plane".parse::<RuntimeMode>().unwrap(),
            RuntimeMode::Gateway
        );
        assert_eq!(
            "controlplane".parse::<RuntimeMode>().unwrap(),
            RuntimeMode::ControlPlane
        );
        assert_eq!(
            "worker".parse::<RuntimeMode>().unwrap(),
            RuntimeMode::Worker
        );
        assert!("bad".parse::<RuntimeMode>().is_err());
    }

    #[test]
    fn mode_capabilities() {
        assert!(RuntimeMode::All.serves_http());
        assert!(RuntimeMode::All.runs_workers());
        assert!(RuntimeMode::Gateway.serves_http());
        assert!(!RuntimeMode::Gateway.runs_workers());
        assert!(RuntimeMode::ControlPlane.serves_http());
        assert!(!RuntimeMode::ControlPlane.runs_workers());
        assert!(!RuntimeMode::Worker.serves_http());
        assert!(RuntimeMode::Worker.runs_workers());
    }
}
