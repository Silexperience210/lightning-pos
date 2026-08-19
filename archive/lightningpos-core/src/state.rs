/// Device state machine — the heart of LightningPoS
///
/// Models all possible device states and defines valid transitions.
/// This is a pure state machine with no side effects — hardware actions
/// are triggered by the firmware layer observing state changes.
///
/// # State Diagram
///
/// ```text
///                    ┌─────────────┐
///                    │ INITIALIZING│──── power on ────┐
///                    └──────┬──────┘                  │
///                           │ init done               │
///                    ┌──────▼──────┐                  │
///              ┌─────│CONNECTING   │◄── WiFi lost ────┤
///              │     │   _WIFI     │                  │
///              │     └──────┬──────┘                  │
///              │    connected│                         │
///              │     ┌──────▼──────┐                  │
///     WiFi lost│     │   READY     │── screen timeout──┤
///              │     └──┬──┬──┬───┘                  │
///              │        │  │  │                       │
///              │   ┌────┘  │  └───────┐              │
///              │   │pay    │help/config│              │
///              │   ▼       ▼           ▼              │
///              │ ┌────────┐┌────────┐┌───────┐       │
///              │ │RECEIVING││  HELP  ││CONFIG │       │
///              │ │PAYMENT ││ SCREEN ││ MODE  │       │
///              │ └───┬────┘└────┬───┘└───┬───┘       │
///              │     │paid      │back     │restart    │
///              │     └──────────┴─────────┘           │
///              │                                      │
///              │  ┌──────────┐  ┌──────────┐         │
///              └──┤SCREENSAVER│◄─┤DEEP_SLEEP│◄────────┘
///                 └──────────┘  └──────────┘
///                    wake on       wake on
///                    touch/btn     GPIO/btn
/// ```
///
/// # Concurrency
///
/// The state machine itself is `Copy + Clone + PartialEq + Eq` — it can be
/// safely shared between tasks via `embassy_sync::Signal` or a mutex.
/// The `StateMachine` wrapper adds transition validation and timestamps.

use core::fmt;

/// All possible device operating states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Power-on, hardware init, config loading
    Initializing,
    /// WiFi connection in progress
    ConnectingWifi,
    /// Fully operational, displaying QR code
    Ready,
    /// QR shown, awaiting Lightning payment
    ReceivingPayment,
    /// Screensaver active (display off, low power)
    Screensaver,
    /// Help/documentation pages
    HelpScreen,
    /// Diagnostic report screen
    ReportScreen,
    /// Serial/web config editor
    ConfigMode,
    /// Critical error (WiFi down, server unreachable)
    ErrorCritical,
    /// Recoverable error (temporary issue)
    ErrorRecoverable,
    /// Deep sleep (ultra-low power)
    DeepSleep,
    /// Multi-product selection screen
    ProductSelection,
    /// Bitcoin price ticker display
    BtcTicker,
}

impl DeviceState {
    /// Human-readable name for logging
    pub fn name(&self) -> &'static str {
        match self {
            Self::Initializing => "INITIALIZING",
            Self::ConnectingWifi => "CONNECTING_WIFI",
            Self::Ready => "READY",
            Self::ReceivingPayment => "RECEIVING_PAYMENT",
            Self::Screensaver => "SCREENSAVER",
            Self::HelpScreen => "HELP_SCREEN",
            Self::ReportScreen => "REPORT_SCREEN",
            Self::ConfigMode => "CONFIG_MODE",
            Self::ErrorCritical => "ERROR_CRITICAL",
            Self::ErrorRecoverable => "ERROR_RECOVERABLE",
            Self::DeepSleep => "DEEP_SLEEP",
            Self::ProductSelection => "PRODUCT_SELECTION",
            Self::BtcTicker => "BTC_TICKER",
        }
    }

    /// Whether the device can receive payments in this state
    pub fn can_receive_payment(&self) -> bool {
        matches!(self, Self::Ready | Self::ReceivingPayment | Self::BtcTicker)
    }

    /// Whether the device is in an error state
    pub fn is_error(&self) -> bool {
        matches!(self, Self::ErrorCritical | Self::ErrorRecoverable)
    }

    /// Whether the device is in a power-saving state
    pub fn is_power_saving(&self) -> bool {
        matches!(self, Self::Screensaver | Self::DeepSleep)
    }
}

/// WiFi connectivity state (orthogonal to DeviceState)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// Result of a state transition attempt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateTransition {
    /// Transition accepted
    Ok,
    /// Already in target state (no-op)
    AlreadyThere,
    /// Transition rejected (invalid)
    Rejected,
}

/// Validates and executes state transitions
#[derive(Debug, Clone)]
pub struct StateMachine {
    current: DeviceState,
    previous: DeviceState,
    wifi: WifiState,
    /// Timestamp (ms) when current state was entered
    entered_at: u64,
}

impl StateMachine {
    pub fn new() -> Self {
        Self {
            current: DeviceState::Initializing,
            previous: DeviceState::Initializing,
            wifi: WifiState::Disconnected,
            entered_at: 0,
        }
    }

    pub fn current(&self) -> DeviceState {
        self.current
    }

    pub fn previous(&self) -> DeviceState {
        self.previous
    }

    pub fn wifi_state(&self) -> WifiState {
        self.wifi
    }

    /// Time spent in current state (milliseconds, requires `now` from caller)
    pub fn time_in_state(&self, now: u64) -> u64 {
        now.saturating_sub(self.entered_at)
    }

    /// Attempt a state transition. Returns the result and the *previous* state
    /// on success (so callers can react to the change).
    pub fn transition(
        &mut self,
        to: DeviceState,
        now: u64,
    ) -> (StateTransition, DeviceState) {
        // No-op if already in this state
        if self.current == to {
            return (StateTransition::AlreadyThere, self.current);
        }

        // Validate transition
        if !self.is_valid_transition(to) {
            return (StateTransition::Rejected, self.current);
        }

        // Execute
        let from = self.current;
        self.previous = from;
        self.current = to;
        self.entered_at = now;

        (StateTransition::Ok, from)
    }

    /// Force a transition (skips validation). Use sparingly — for reset/wake.
    pub fn force_transition(&mut self, to: DeviceState, now: u64) -> DeviceState {
        let from = self.current;
        self.previous = from;
        self.current = to;
        self.entered_at = now;
        from
    }

    /// Update WiFi state. May trigger automatic device state transitions
    /// (e.g., WiFi lost → ConnectingWifi, WiFi restored → Ready).
    pub fn update_wifi(
        &mut self,
        new_wifi: WifiState,
        now: u64,
    ) -> Option<(StateTransition, DeviceState)> {
        self.wifi = new_wifi;

        // WiFi lost during critical states
        if matches!(new_wifi, WifiState::Disconnected | WifiState::Error) {
            if matches!(
                self.current,
                DeviceState::Ready
                    | DeviceState::ReceivingPayment
                    | DeviceState::BtcTicker
            ) {
                let (result, from) = self.transition(DeviceState::ConnectingWifi, now);
                return Some((result, from));
            }
        }

        // WiFi restored
        if new_wifi == WifiState::Connected {
            if self.current == DeviceState::ConnectingWifi {
                let (result, from) = self.transition(DeviceState::Ready, now);
                return Some((result, from));
            }
        }

        None
    }

    /// Validate a state transition
    fn is_valid_transition(&self, to: DeviceState) -> bool {
        use DeviceState::*;

        // Can't leave DeepSleep except via Initializing
        if self.current == DeepSleep {
            return to == Initializing;
        }

        // ConfigMode is unbreakable — only ESP.restart() exits
        if self.current == ConfigMode {
            return to == Initializing;
        }

        // Can't enter DeepSleep except from Ready or Screensaver
        if to == DeepSleep {
            return matches!(self.current, Ready | Screensaver);
        }

        // WiFi failure can jump to ConnectingWifi from most states
        if to == ConnectingWifi {
            return !matches!(self.current, ConfigMode | ErrorCritical | DeepSleep);
        }

        // Can't escape ErrorCritical except via Initializing
        if self.current == ErrorCritical {
            return to == Initializing;
        }

        // Most other transitions are valid
        true
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_boot_sequence() {
        let mut sm = StateMachine::new();
        let now = 1000;

        assert_eq!(sm.current(), DeviceState::Initializing);

        let (r, _) = sm.transition(DeviceState::ConnectingWifi, now);
        assert_eq!(r, StateTransition::Ok);
        assert_eq!(sm.current(), DeviceState::ConnectingWifi);

        let (r, _) = sm.transition(DeviceState::Ready, now);
        assert_eq!(r, StateTransition::Ok);
        assert_eq!(sm.current(), DeviceState::Ready);
    }

    #[test]
    fn test_payment_flow() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::Ready, now);
        let (r, _) = sm.transition(DeviceState::ReceivingPayment, now);
        assert_eq!(r, StateTransition::Ok);

        // Payment received → back to Ready
        let (r, _) = sm.transition(DeviceState::Ready, now);
        assert_eq!(r, StateTransition::Ok);
    }

    #[test]
    fn test_config_mode_is_escape_hatch() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::Ready, now);
        let (r, _) = sm.transition(DeviceState::ConfigMode, now);
        assert_eq!(r, StateTransition::Ok);

        // From config, only Initializing is valid
        let (r, _) = sm.transition(DeviceState::Ready, now);
        assert_eq!(r, StateTransition::Rejected);
        let (r, _) = sm.transition(DeviceState::Initializing, now);
        assert_eq!(r, StateTransition::Ok);
    }

    #[test]
    fn test_wifi_lost_triggers_reconnect() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::Ready, now);
        let result = sm.update_wifi(WifiState::Disconnected, now);
        assert!(result.is_some());
        assert_eq!(sm.current(), DeviceState::ConnectingWifi);
    }

    #[test]
    fn test_wifi_restored_triggers_ready() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::ConnectingWifi, now);
        let result = sm.update_wifi(WifiState::Connected, now);
        assert!(result.is_some());
        assert_eq!(sm.current(), DeviceState::Ready);
    }

    #[test]
    fn test_deep_sleep_guard() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::Ready, now);
        let (r, _) = sm.transition(DeviceState::DeepSleep, now);
        assert_eq!(r, StateTransition::Ok);

        // Can only wake via Initializing
        let (r, _) = sm.transition(DeviceState::Ready, now);
        assert_eq!(r, StateTransition::Rejected);
        let (r, _) = sm.transition(DeviceState::Initializing, now);
        assert_eq!(r, StateTransition::Ok);
    }

    #[test]
    fn test_noop_transition() {
        let mut sm = StateMachine::new();
        let now = 1000;

        sm.force_transition(DeviceState::Ready, now);
        let (r, _) = sm.transition(DeviceState::Ready, now);
        assert_eq!(r, StateTransition::AlreadyThere);
    }
}
