//! UPower battery provider.
//!
//! Subscribes to battery state via `org.freedesktop.UPower` service,
//! reading properties from the DisplayDevice.

use std::collections::HashMap;

use async_trait::async_trait;
use futures::stream::BoxStream;
use zbus::zvariant::OwnedValue;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, BatteryChargeState, DomainError, Match, PowerState,
    ProviderCapabilities, ProviderId, ProviderSource, Query,
};

use quantum_dbus::DbusError;

use crate::error::InfrastructureError;

pub struct UpowerBatteryProvider {
    id: ProviderId,
    conn: Option<Connection>,
    available: bool,
}

impl UpowerBatteryProvider {
    /// Attempt to connect to UPower on the system bus.
    ///
    /// If the system bus is unavailable, returns `Ok(Self { ... available: false })`
    /// with no error — the provider degrades gracefully. If the bus is available
    /// but UPower is not, marks `available: false` and continues.
    pub async fn connect() -> Result<Self, InfrastructureError> {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(_e) => {
                // System bus unavailable — this is not an error from the provider's
                // perspective; it's a graceful fallback.
                return Ok(Self {
                    id: ProviderId::from("power"),
                    conn: None,
                    available: false,
                });
            }
        };

        let available =
            quantum_dbus::common::service_available(&conn, "org.freedesktop.UPower").await;

        Ok(Self {
            id: ProviderId::from("power"),
            conn: Some(conn),
            available,
        })
    }
}

#[async_trait]
impl ProviderSource for UpowerBatteryProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: false,
            streamable: true,
        }
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, _: &Action) -> Result<ActionOutcome, DomainError> {
        Err(DomainError::Unsupported(
            "power provider is read-only".to_string(),
        ))
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        if !self.available || self.conn.is_none() {
            return Some(quantum_dbus::common::unavailable_stream::<PowerState>());
        }

        let conn = self.conn.as_ref().unwrap().clone();

        let build: quantum_dbus::common::BuildFn<PowerState> = Box::new(|conn: &Connection| {
            Box::pin(async {
                let proxy = zbus::Proxy::new(
                    conn,
                    "org.freedesktop.UPower",
                    "/org/freedesktop/UPower/devices/DisplayDevice",
                    "org.freedesktop.UPower.Device",
                )
                .await
                .map_err(|e| DbusError::Transport(e.to_string()))?;

                let percentage: f64 = proxy.get_property("Percentage").await.unwrap_or(0.0);
                let state: u32 = proxy.get_property("State").await.unwrap_or(0);
                let time_to_empty: i64 = proxy.get_property("TimeToEmpty").await.unwrap_or(0);
                let time_to_full: i64 = proxy.get_property("TimeToFull").await.unwrap_or(0);
                let is_present: bool = proxy.get_property("IsPresent").await.unwrap_or(false);

                let mut props = HashMap::new();
                props.insert("Percentage".to_string(), OwnedValue::from(percentage));
                props.insert("State".to_string(), OwnedValue::from(state));
                props.insert("TimeToEmpty".to_string(), OwnedValue::from(time_to_empty));
                props.insert("TimeToFull".to_string(), OwnedValue::from(time_to_full));
                props.insert("IsPresent".to_string(), OwnedValue::from(is_present));

                Ok(map_upower_props(&props))
            })
        });

        Some(quantum_dbus::common::property_subscription_stream(
            conn,
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower/devices/DisplayDevice",
            "org.freedesktop.UPower.Device",
            build,
        ))
    }
}

/// Map UPower properties dict to PowerState.
///
/// This is a pure function for testability. Missing keys or failed downcasts
/// are treated as absent — the function never panics.
pub(crate) fn map_upower_props(props: &HashMap<String, OwnedValue>) -> PowerState {
    let percentage = props
        .get("Percentage")
        .and_then(|v| f64::try_from(v).ok())
        .and_then(|f| {
            if (0.0..=100.0).contains(&f) {
                Some(f as f32)
            } else {
                None
            }
        });

    let state = props
        .get("State")
        .and_then(|v| u32::try_from(v).ok())
        .map(|s| match s {
            1 => BatteryChargeState::Charging,
            2 => BatteryChargeState::Discharging,
            3 => BatteryChargeState::Empty,
            4 => BatteryChargeState::Full,
            _ => BatteryChargeState::Unknown,
        });

    let on_battery = props
        .get("State")
        .and_then(|v| u32::try_from(v).ok())
        .map(|s| s == 2)
        .unwrap_or(false);

    let time_to_empty_secs = props
        .get("TimeToEmpty")
        .and_then(|v| i64::try_from(v).ok())
        .and_then(|t| if t > 0 { Some(t as u64) } else { None });

    let time_to_full_secs = props
        .get("TimeToFull")
        .and_then(|v| i64::try_from(v).ok())
        .and_then(|t| if t > 0 { Some(t as u64) } else { None });

    let available = props
        .get("IsPresent")
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false);

    PowerState {
        available,
        on_battery,
        percentage,
        state,
        time_to_empty_secs,
        time_to_full_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_upower_discharging_50_percent() {
        let mut props = HashMap::new();
        props.insert("Percentage".to_string(), OwnedValue::from(50.0_f64));
        props.insert("State".to_string(), OwnedValue::from(2_u32));
        props.insert("TimeToEmpty".to_string(), OwnedValue::from(3600_i64));
        props.insert("TimeToFull".to_string(), OwnedValue::from(0_i64));
        props.insert("IsPresent".to_string(), OwnedValue::from(true));

        let s = map_upower_props(&props);
        assert!(s.available);
        assert!(s.on_battery);
        assert_eq!(s.percentage, Some(50.0));
        assert_eq!(s.state, Some(BatteryChargeState::Discharging));
        assert_eq!(s.time_to_empty_secs, Some(3600));
        assert_eq!(s.time_to_full_secs, None);
    }

    #[test]
    fn maps_upower_charging_80_percent() {
        let mut props = HashMap::new();
        props.insert("Percentage".to_string(), OwnedValue::from(80.0_f64));
        props.insert("State".to_string(), OwnedValue::from(1_u32));
        props.insert("TimeToEmpty".to_string(), OwnedValue::from(0_i64));
        props.insert("TimeToFull".to_string(), OwnedValue::from(1800_i64));
        props.insert("IsPresent".to_string(), OwnedValue::from(true));

        let s = map_upower_props(&props);
        assert!(s.available);
        assert!(!s.on_battery);
        assert_eq!(s.percentage, Some(80.0));
        assert_eq!(s.state, Some(BatteryChargeState::Charging));
        assert_eq!(s.time_to_empty_secs, None);
        assert_eq!(s.time_to_full_secs, Some(1800));
    }

    #[test]
    fn maps_upower_not_present() {
        let mut props = HashMap::new();
        props.insert("IsPresent".to_string(), OwnedValue::from(false));

        let s = map_upower_props(&props);
        assert!(!s.available);
    }

    #[test]
    fn maps_upower_empty_dict_is_default() {
        let props = HashMap::new();

        let s = map_upower_props(&props);
        assert_eq!(s, PowerState::default());
    }

    #[tokio::test]
    #[ignore = "requires real UPower on system bus"]
    async fn yields_initial_state_within_2s() {
        use futures::StreamExt;
        use std::time::Duration;

        let p = UpowerBatteryProvider::connect().await.expect("connect");
        let mut stream = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("first state within 2s")
            .expect("Some");
        let _state: PowerState = serde_json::from_value(v).expect("PowerState");
    }
}
